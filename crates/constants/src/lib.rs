use crate::version::Version;

pub mod version;

pub const CONTROLLER_HOST: &str = "service.w3flo.com";
pub const CONTROLLER_GRPC_PORT: u16 = 3549;
pub const CONTROLLER_SOCKET_PORT: u16 = 3550;
pub const CLIENT_WS_PORT: u16 = 3551;
pub const CLIENT_ORIGINS: &[&str] = &[
  "http://localhost:3000",
  "https://w3flo.com",
  "https://asia.w3flo.com",
];
pub const NODE_PORT: u16 = 3552;
pub const NODE_ECHO_PORT: u16 = 3552;
pub const NODE_ECHO_PORT_OFFSET: u16 = NODE_ECHO_PORT - NODE_PORT;
pub const NODE_CONTROLLER_PORT: u16 = 3553;
pub const NODE_CONTROLLER_PORT_OFFSET: u16 = NODE_CONTROLLER_PORT - NODE_ECHO_PORT;
pub const NODE_CLIENT_PORT: u16 = 3554;
pub const NODE_CLIENT_PORT_OFFSET: u16 = NODE_CLIENT_PORT - NODE_ECHO_PORT;
pub const NODE_HTTP_PORT: u16 = 3555;
pub const NODE_HTTP_PORT_OFFSET: u16 = NODE_HTTP_PORT - NODE_ECHO_PORT;
pub const MIN_FLO_VERSION: version::Version = Version {
  major: 0,
  minor: 9,
  patch: 2,
};
