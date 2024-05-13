// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use crossbeam_channel::{unbounded, Sender};
#[cfg(target_os = "macos")]
use macos_accessibility_client;
use rdev::{display_size, exit_grab, grab, Event, EventType};
use std::{
    collections::HashMap,
    io::prelude::*,
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};
use tauri::{AppHandle, Manager};

#[tauri::command]
fn start_grab(app_handle: AppHandle, sender: tauri::State<SenderState>) {
    let sender = sender.0.clone();

    thread::spawn(move || {
        #[cfg(target_os = "macos")]
        macos_accessibility_client::accessibility::application_is_trusted_with_prompt();

        let h = app_handle.clone();
        #[cfg(target_os = "macos")]
        rdev::set_is_main_thread(false);
        // FIXME: keyboard captures only when main window is unfocused
        #[cfg(target_os = "windows")]
        rdev::set_event_popup(false);
        grab(move |event| {
            let _ = h.emit_all("event", &event);
            sender.try_send(event.clone()).unwrap();
            match event.event_type {
                // EventType::KeyPress(_) => None,
                _ => Some(event),
            }
        })
        .expect("could not listen events");
    });
}

#[tauri::command]
fn stop_grab() {
    exit_grab().expect("Could not stop grab");
}

#[tauri::command]
fn test() {
    let stream = Arc::new(Mutex::new(TcpStream::connect("localhost:1337").unwrap()));
    stream.lock().unwrap().write_all(b"1").unwrap();
    let s = Arc::clone(&stream);
    /*TODO:
     * - get current screen size
     * - send it to the server
     * - figure out how to calculate mouse movement
     * */

    let mut buffer = [0; 128];
    thread::spawn(move || loop {
        match stream.lock().unwrap().read(&mut buffer) {
            Ok(n) => {
                if n > 0 {
                    let message: String = String::from_utf8_lossy(&buffer[..n]).into();
                    println!("Message from server: {message}")
                } else {
                    println!("break");
                    break;
                }
            }

            Err(e) => {
                println!("ErrorKind: {e}");
                break;
            }
        }
    });

    // thread::spawn(move || {
    //     thread::sleep(Duration::from_secs(2));
    //     s.lock().unwrap().shutdown(std::net::Shutdown::Both);
    // });
}

struct SenderState(Sender<Event>);

fn main() {
    let (send, recv) = unbounded();
    let clients: Arc<Mutex<HashMap<std::net::SocketAddr, Arc<TcpStream>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let c = Arc::clone(&clients);

    thread::spawn(move || {
        for m in recv {
            let json = serde_json::to_string(&m).expect("Could not stringify json");
            for s in clients.lock().unwrap().values() {
                s.as_ref()
                    .write_all(json.as_bytes())
                    .expect("Could not write to stream");
            }
        }
        println!("Connection Closed");
    });

    thread::spawn(move || {
        let listener = TcpListener::bind("localhost:1337").expect("could not start tcp listener");
        println!("Server Started...");

        let mut buffer = [0; 128];

        for stream in listener.incoming() {
            let c = Arc::clone(&c);

            let mut stream = stream.expect("some stream error??");

            c.lock().unwrap().insert(
                stream.peer_addr().unwrap(),
                Arc::new(stream.try_clone().unwrap()),
            );

            println!("{c:?}");

            println!("New Connection");

            thread::spawn(move || {
                loop {
                    match stream.read(&mut buffer) {
                        Ok(n) => {
                            if n > 0 {
                                let message: String = String::from_utf8_lossy(&buffer[..n]).into();
                                println!("Message from client: {message}")
                            } else {
                                break;
                            }
                        }

                        Err(e) => {
                            println!("ErrorKind: {e}");
                            break;
                        }
                    }
                }
                let mut c = c.lock().unwrap();
                c.remove(&stream.peer_addr().unwrap());

                if c.len() == 0 {
                    exit_grab().expect("Could not stop grab");
                }

                println!("Connection Closed")
            });
        }

        println!("Server Stopped...");
    });

    tauri::Builder::default()
        .manage(SenderState(send))
        .invoke_handler(tauri::generate_handler![test, start_grab, stop_grab])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
