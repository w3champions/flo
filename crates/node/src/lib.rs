mod client;
mod controller;
mod echo;
mod env;
mod game;
mod metrics;
mod state;
mod version;

mod constants;
pub mod error;

use error::Result;

use flo_event::*;

use self::client::serve_client;
use self::echo::serve_echo;
use self::metrics::serve_metrics;
use crate::state::GlobalState;
use state::event::{handle_global_events, FloNodeEventContext, GlobalEvent};

pub async fn serve() -> Result<()> {
  let (event_sender, event_receiver) = GlobalEvent::channel(30);
  let state = GlobalState::new(event_sender).into_ref();
  let mut ctrl = controller::ControllerServer::new(state.clone());
  let ctrl_handle = ctrl.handle();

  tokio::try_join!(
    ctrl.serve(),
    serve_client(state.clone()),
    serve_metrics(),
    serve_echo(),
    handle_global_events(
      FloNodeEventContext {
        state,
        ctrl: ctrl_handle,
      },
      event_receiver
    )
  )
  .map(|_| ())
}
