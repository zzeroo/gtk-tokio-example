// Stolen from her: https://git.sr.ht/~azdle/nex-trip-gtk/tree/c8f04fcd933d9ddeeec47176e59db8e48efd8abd/src/main.rs

use futures::channel::mpsc::{Receiver, Sender};
use gio::prelude::*;
use gtk::prelude::*;
use gtk::{ApplicationWindow, Builder, Button, Label};
use serde::Deserialize;
use std::cell::RefCell;
use std::env::args;
use std::rc::Rc;

#[derive(Debug)]
enum UiEvent {
    Refresh,
}

#[derive(Debug)]
enum DataEvent {
    UpdateInfo(String),
}

#[derive(Clone)]
struct UiElements {
    info_label: Label,
}

fn build_ui(
    application: &gtk::Application,
    ui_event_sender: Sender<UiEvent>,
    data_event_receiver: Rc<RefCell<Option<Receiver<DataEvent>>>>,
) {
    let glade_src = include_str!("main.glade");
    let builder = Builder::new_from_string(glade_src);

    let window: ApplicationWindow = builder.get_object("main-window").expect("get main-window");
    window.set_application(Some(application));

    // info stuff
    let refresh_button: Button = builder
        .get_object("refresh-button")
        .expect("get refresh-button");
    let refresh_button2: Button = builder
        .get_object("refresh-button2")
        .expect("get refresh-button2");
    let info_label: Label = builder.get_object("info-label").expect("get info-label");

    {
        let info_label = info_label.clone();
        refresh_button.connect_clicked(move |_| {
            ui_event_sender
                .clone()
                .try_send(UiEvent::Refresh)
                .expect("send UI event from refresh button");
            info_label.set_text("Fetching...");
        });
    }

    // **This triggers**: `futures_channel::mpsc::Sender<UiEvent>`, which does not implement the `Copy` trait 
    // {
    //     let info_label = info_label.clone();
    //     refresh_button2.connect_clicked(move |_| {
    //         ui_event_sender
    //             .clone()
    //             .try_send(UiEvent::Refresh)
    //             .expect("send UI event from refresh button");
    //         info_label.set_text("Fetching...");
    //     });
    // }

    window.show_all();

    let ui_elements = UiElements { info_label };

    // future on main thread that has access to UI
    let future = {
        let info_label = ui_elements.info_label.clone();
        let mut data_event_receiver = data_event_receiver
            .replace(None)
            .take()
            .expect("data_event_reciver");
        async move {
            use futures::stream::StreamExt;

            while let Some(event) = data_event_receiver.next().await {
                println!("data event: {:?}", event);
                match event {
                    DataEvent::UpdateInfo(text) => info_label.set_text(&text),
                }
            }
        }
    };

    let c = glib::MainContext::default();
    c.spawn_local(future);

    // do I need to put this somewhere? It doesn't seem to do anything and doesn't cause problems
    // if I don't use it.
    //c.pop_thread_default();
}

async fn fetch_next_departure() -> String {
    let resp: Vec<Departure> =
        reqwest::get("https://svc.metrotransit.org/NexTrip/56026?format=json")
            .await
            .expect("fetch info")
            .json()
            .await
            .expect("parse body as json");
    println!("{:#?}", resp);

    resp[0].departure_text.clone()
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct Departure {
    departure_text: String,
}

fn main() {
    use std::thread;

    // thread-to-thread communication
    let (ui_event_sender, mut ui_event_receiver) = futures::channel::mpsc::channel(0);
    let (mut data_event_sender, data_event_receiver) = futures::channel::mpsc::channel(0);

    // spawn data/network thread
    thread::spawn(move || {
        use tokio::runtime::Runtime;

        let mut rt = Runtime::new().expect("create tokio runtime");
        rt.block_on(async {
            use futures::sink::SinkExt;
            use futures::stream::StreamExt;

            while let Some(event) = ui_event_receiver.next().await {
                println!("got event: {:?}", event);
                match event {
                    UiEvent::Refresh => data_event_sender
                        .send(DataEvent::UpdateInfo(fetch_next_departure().await))
                        .await
                        .expect("send refresh result"),
                }
            }
        })
    });

    // main thread is ui thread
    let application = gtk::Application::new(Some("net.azdle.nex-trip-gtk"), Default::default())
        .expect("create application");

    // this type is a evil hack that I'm using because the closure passed to `connect_activate`
    // needs to be `Clone`.
    let data_event_receiver = Rc::new(RefCell::new(Some(data_event_receiver)));
    application.connect_activate(move |app| {
        build_ui(app, ui_event_sender.clone(), data_event_receiver.clone());
    });

    application.run(&args().collect::<Vec<_>>());
}
