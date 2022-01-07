use flo_grpc::{
  Channel,
  controller::flo_controller_client::FloControllerClient,
};
use s2_grpc_utils::S2ProtoUnpack;
use tonic::{service::Interceptor, metadata::{MetadataValue, Ascii}, codegen::InterceptedService};
use once_cell::sync::Lazy;
use crate::error::{Result, Error};
use crate::game::Game;

type Client = FloControllerClient<InterceptedService<Channel, WithSecretInterceptor>>;

#[derive(Debug, Clone)]
pub struct Controller {
  client: Client,
}

impl Controller {
  pub fn env() -> Self {
    let chan = Channel::from_static(crate::env::ENV.controller_url.as_str());
    let secret = crate::env::ENV.controller_secret.parse().unwrap();
    Self {
      client: FloControllerClient::with_interceptor(chan.connect_lazy(), WithSecretInterceptor {
        secret,
    }),
    }
  }

  pub async fn fetch_game(&self, game_id: i32) -> Result<Game> {
    use flo_grpc::controller::GetGameRequest;
    let res = self.client.clone().get_game(GetGameRequest {
      game_id
    }).await;
    match res {
      Ok(res) => Ok(Game::unpack(res.into_inner().game)?),
      Err(status) => {
        if status.code() == tonic::Code::InvalidArgument {
          Err(Error::InvalidGameId(game_id))
        } else {
          Err(Error::ControllerService(status))
        }
      },
    }
  }
}

#[derive(Clone)]
pub struct WithSecretInterceptor {
  secret: MetadataValue<Ascii>,
}

impl Interceptor for WithSecretInterceptor {
  fn call(&mut self, mut request: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
    request
      .metadata_mut()
      .insert("x-flo-secret", self.secret.clone());
    Ok(request)
  }
}

#[tokio::test]
async fn test_ctrlr_client() -> anyhow::Result<()> {
  dotenv::dotenv().unwrap();
  let client = Controller::env();
  let game = client.fetch_game(2048816).await?;
  dbg!(game);
  Ok(())
}
