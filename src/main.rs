use tokio::fs::remove_file;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;
use tokio::task;

use time;

use crossterm::event;

enum MainLoopMessage {
    AllOk,
    SocketMessage(String, String),
    NewConnection(String, mpsc::Sender<MainToConnectionMessage>),
    ConnectionDropped(String),
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

    let mut all_senders = std::collections::HashMap::new();

    let mut app = App::new();

    let mut terminal = ratatui::init();
    loop {
        terminal
            .draw(|frame| {
                app.draw(frame);
            })
            .unwrap();

        let mut break_loop = false;
        let message = connection_to_main_receiver.recv().await;

        match message.unwrap() {
            MainLoopMessage::SocketMessage(sender, msg) => {
                app.add_connection_message(sender, msg);
            }
            MainLoopMessage::NewConnection(socket_name, sender) => {
                all_senders.insert(socket_name.clone(), sender);
                app.current_jobs.push(CurrentJob {
                    job_name: socket_name,
                    messages: Vec::new(),
                })
            }
            MainLoopMessage::ConnectionDropped(sender) => {
                app.finish_connection(sender);
            }
            MainLoopMessage::KeyPressed(key_code) => match key_code {
                event::KeyCode::Backspace => {
                    app.current_input.pop();
                }
                event::KeyCode::Enter => break_loop = true,
                event::KeyCode::Char(x) => {
                    app.current_input.push(x);
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

    let mut buffer: [u8; 500] = [0; 500];
    let n_bytes = stream.read(&mut buffer).await.unwrap();
    let socket_name = std::str::from_utf8(&buffer[..n_bytes]).unwrap().to_owned();

    connection_to_main_sender
        .send(MainLoopMessage::NewConnection(
            socket_name.clone(),
            main_to_connection_sender,
        ))
        .await
        .unwrap();

    let mut buffer: [u8; 1024] = [0; 1024];
    loop {
        let break_loop = tokio::select! {
            n_bytes = stream.read(&mut buffer) => {
                let n_bytes = n_bytes.unwrap();
                let message = std::str::from_utf8(&buffer[..n_bytes]).unwrap().to_owned();
                if message == "FINISH" {
                    connection_to_main_sender
                        .send(MainLoopMessage::ConnectionDropped(socket_name.clone()))
                        .await
                        .unwrap();
                    true
                } else {
                    connection_to_main_sender
                        .send(MainLoopMessage::SocketMessage(socket_name.clone(), message))
                        .await
                        .unwrap();
                    false
                }
            },
            message = main_to_connection_receiver.recv() => {
                match message.unwrap() {
                    MainToConnectionMessage::AllOk => false,
                    MainToConnectionMessage::CancelJob => {
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

struct FinishedJob {
    job_name: String,
    end_time: String,
}

struct CurrentJob {
    job_name: String,
    messages: Vec<String>,
}

struct App {
    current_input: String,
    finished_jobs: Vec<FinishedJob>,
    current_jobs: Vec<CurrentJob>,
}

impl App {
    fn new() -> Self {
        App {
            current_input: String::new(),
            finished_jobs: Vec::new(),
            current_jobs: Vec::new(),
        }
    }

    fn add_new_connection(&mut self, connection_name: String) {
        self.current_jobs.push(CurrentJob {
            job_name: connection_name,
            messages: Vec::new(),
        })
    }

    fn add_connection_message(&mut self, connection_name: String, message: String) {
        for job in self.current_jobs.iter_mut() {
            if job.job_name == connection_name {
                job.messages.push(message);
                break;
            }
        }
    }

    fn finish_connection(&mut self, connection_name: String) {
        let mut index_to_remove = None;
        for (i, job_name) in self.current_jobs.iter().map(|x| &x.job_name).enumerate() {
            if job_name == &connection_name {
                index_to_remove = Some(i);
                break;
            }
        }
        self.current_jobs.remove(index_to_remove.unwrap());

        let end_time = time::OffsetDateTime::now_utc();

        self.finished_jobs.push(FinishedJob {
            job_name: connection_name.clone(),
            end_time: format!("{:0>2}:{:0>2}", end_time.hour(), end_time.minute()),
        });
    }

    fn draw(&self, frame: &mut ratatui::Frame) {
        let [main_area, bottom_area] = ratatui::layout::Layout::vertical([
            ratatui::layout::Constraint::Min(1),
            ratatui::layout::Constraint::Max(2),
        ])
        .areas(frame.area());

        let finished_jobs_height = 7 + self.finished_jobs.len().min(5) as u16;

        let [finished_jobs_area, _, current_jobs_area] = ratatui::layout::Layout::vertical([
            ratatui::layout::Constraint::Max(finished_jobs_height),
            ratatui::layout::Constraint::Max(3),
            ratatui::layout::Constraint::Min(3),
        ])
        .areas(main_area);

        let finished_padding = ratatui::widgets::block::Padding::new(3, 0, 1, 1);

        let finished_block = ratatui::widgets::Block::bordered()
            .padding(finished_padding)
            .title("Finished Jobs");

        let rows: Vec<ratatui::widgets::Row> = self
            .finished_jobs
            .iter()
            .enumerate()
            .map(|(i, x)| {
                ratatui::widgets::Row::new(vec![
                    format!("{}", i + 1),
                    x.job_name.clone(),
                    x.end_time.clone(),
                ])
            })
            .collect();

        let finished_jobs_widget =
            ratatui::widgets::Table::new(rows, [3, 15, 15]).block(finished_block);

        frame.render_widget(finished_jobs_widget, finished_jobs_area);

        let current_jobs_split_areas = ratatui::layout::Layout::vertical(
            std::iter::repeat(ratatui::layout::Constraint::Max(15)).take(self.current_jobs.len()),
        )
        .split(current_jobs_area);

        for (i, job) in self.current_jobs.iter().enumerate() {
            let finished_padding = ratatui::widgets::block::Padding::new(3, 0, 1, 1);

            let finished_block = ratatui::widgets::Block::bordered()
                .padding(finished_padding)
                .title(self.current_jobs[i].job_name.to_owned());

            let job_messages =
                ratatui::widgets::List::new(job.messages.clone()).block(finished_block);

            frame.render_widget(job_messages, current_jobs_split_areas[i])
        }

        frame.render_widget(
            ratatui::widgets::Paragraph::new(format!("{}|", self.current_input).as_str()),
            bottom_area,
        );
    }
}
