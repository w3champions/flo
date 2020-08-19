use tokio::sync::mpsc::{Receiver, Sender};

use flo_net::packet::Frame;

#[derive(Debug, Clone)]
pub struct PlayerSender {
  sender: Sender<Frame>,
}

#[derive(Debug)]
pub struct PlayerReceiver {
  receiver: Receiver<Frame>,
}
