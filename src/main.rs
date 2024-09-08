use tokio::fs::remove_file;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;
use tokio::task;

enum MainLoopMessage {
    AllOk,
    Message(String),
    NewConnection(mpsc::Sender<MainToConnectionMessage>),
}

enum MainToConnectionMessage {
    AllOk,
    CancelJob,
}

enum MainToListenerMessage {
    Shutdown,
}

#[tokio::main]
async fn main() {
    println!("Hello, world!");

    let (connection_to_main_sender, mut connection_to_main_receiver) =
        mpsc::channel::<MainLoopMessage>(300);

    let (main_to_listener_sender, main_to_listener_receiver) =
        mpsc::channel::<MainToListenerMessage>(300);

    let gate_handle = task::spawn(async move {
        listen_port(main_to_listener_receiver, connection_to_main_sender).await
    });

    loop {
        match connection_to_main_receiver.recv().await.unwrap() {
            MainLoopMessage::Message(msg) => {
                println!("{}", msg);
            }
            _ => break,
        }
    }
    gate_handle.await.unwrap();
}

async fn listen_port(
    mut main_to_listener_receiver: mpsc::Receiver<MainToListenerMessage>,
    connection_to_main_sender: mpsc::Sender<MainLoopMessage>,
) {
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
    println!("Exiting listener");
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
