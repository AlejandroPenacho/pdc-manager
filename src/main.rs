use tokio::fs::remove_file;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;
use tokio::task;

use crossterm::event;

enum MainLoopMessage {
    AllOk,
    Message(String),
    NewConnection(mpsc::Sender<MainToConnectionMessage>),
    KeyPressed(event::KeyCode),
}

enum MainToConnectionMessage {
    AllOk,
    CancelJob,
}

enum MainToListenerMessage {
    Shutdown,
}

enum MainToInterfaceMessage {
    AllOk,
}
enum InterfaceToMainMessage {
    Text(String),
}

#[tokio::main]
async fn main() {
    let (
        interface_handle,
        listener_handle,
        mut connection_to_main_receiver,
        main_to_listener_sender,
    ) = initialize_process();

    let mut all_senders = Vec::new();
    let mut current_input = String::new();

    let mut terminal = ratatui::init();
    loop {
        terminal
            .draw(|frame| {
                let [main_area, bottom_area] = ratatui::layout::Layout::vertical([
                    ratatui::layout::Constraint::Min(1),
                    ratatui::layout::Constraint::Max(2),
                ])
                .areas(frame.area());

                frame.render_widget(
                    ratatui::widgets::Paragraph::new("Everything as expected :)"),
                    main_area,
                );
                frame.render_widget(
                    ratatui::widgets::Paragraph::new(format!("{}|", current_input).as_str()),
                    bottom_area,
                );
            })
            .unwrap();

        let mut break_loop = false;
        let message = connection_to_main_receiver.recv().await;

        match message.unwrap() {
            MainLoopMessage::Message(msg) => {
                println!("From outside: {}", msg);
            }
            MainLoopMessage::NewConnection(sender) => all_senders.push(sender),
            MainLoopMessage::KeyPressed(key_code) => match key_code {
                event::KeyCode::Backspace => {
                    current_input.pop();
                }
                event::KeyCode::Enter => break_loop = true,
                event::KeyCode::Char(x) => {
                    current_input.push(x);
                }
                _ => {}
            },
            _ => {}
        };

        if break_loop {
            break;
        }
    }

    terminal
        .draw(|frame| {
            frame.render_widget(
                ratatui::widgets::Paragraph::new("Press any key to exit"),
                frame.area(),
            )
        })
        .unwrap();

    drop(connection_to_main_receiver);

    main_to_listener_sender
        .send(MainToListenerMessage::Shutdown)
        .await
        .unwrap();
    listener_handle.await.unwrap();
    interface_handle.join().unwrap();
    ratatui::restore();
}

fn initialize_process() -> (
    std::thread::JoinHandle<()>,
    task::JoinHandle<()>,
    mpsc::Receiver<MainLoopMessage>,
    mpsc::Sender<MainToListenerMessage>,
) {
    let (connection_to_main_sender, connection_to_main_receiver) =
        mpsc::channel::<MainLoopMessage>(300);

    let (main_to_listener_sender, main_to_listener_receiver) =
        mpsc::channel::<MainToListenerMessage>(300);

    let interface_to_main_sender = connection_to_main_sender.clone();

    let listener_handle = task::spawn(async move {
        listen_port(main_to_listener_receiver, connection_to_main_sender).await
    });

    let interface_handle = std::thread::spawn(move || interface_loop(interface_to_main_sender));

    return (
        interface_handle,
        listener_handle,
        connection_to_main_receiver,
        main_to_listener_sender,
    );
}

async fn listen_port(
    mut main_to_listener_receiver: mpsc::Receiver<MainToListenerMessage>,
    connection_to_main_sender: mpsc::Sender<MainLoopMessage>,
) {
    if let Err(_) = tokio::fs::remove_file("the_socket").await {};
    let listener = UnixListener::bind("the_socket").unwrap();
    loop {
        let break_loop: bool = tokio::select! {
            stream = listener.accept() => {
                let stream = stream.unwrap().0;
                let cloned_main_sender = connection_to_main_sender.clone();
                task::spawn(async move { handle_connection(stream, cloned_main_sender).await });
                false
            },
            _message = main_to_listener_receiver.recv() => {
                // TODO: Handle more messages?
                true
            }
        };
        if break_loop {
            break;
        }
    }
    remove_file("the_socket").await.ok();
}

async fn handle_connection(
    mut stream: UnixStream,
    connection_to_main_sender: mpsc::Sender<MainLoopMessage>,
) {
    let (main_to_connection_sender, mut main_to_connection_receiver) = mpsc::channel(200);

    connection_to_main_sender
        .send(MainLoopMessage::NewConnection(main_to_connection_sender))
        .await
        .unwrap();

    stream
        .write(&"Hello, place your orders\n".as_bytes())
        .await
        .unwrap();

    let mut buffer: [u8; 1024] = [0; 1024];
    loop {
        let break_loop = tokio::select! {
            n_bytes = stream.read(&mut buffer) => {
                let n_bytes = n_bytes.unwrap();
                let message = std::str::from_utf8(&buffer[..n_bytes]).unwrap().to_owned();
                connection_to_main_sender
                    .send(MainLoopMessage::Message(message))
                    .await
                    .unwrap();
                false
            },
            message = main_to_connection_receiver.recv() => {
                match message.unwrap() {
                    MainToConnectionMessage::AllOk => false,
                    MainToConnectionMessage::CancelJob => {
                        println!("Cancelling");
                        true
                    }
                }
            }
        };
        if break_loop {
            break;
        }
    }
}

fn interface_loop(interface_to_main_sender: mpsc::Sender<MainLoopMessage>) {
    loop {
        if let event::Event::Key(key) = event::read().unwrap() {
            if key.kind != event::KeyEventKind::Press {
                continue;
            }
            if let Err(_) =
                interface_to_main_sender.blocking_send(MainLoopMessage::KeyPressed(key.code))
            {
                break;
            }
        }
    }
}
