// use prost::Message;
//
// #[derive(Debug, Message)]
// pub struct Version {
//   #[prost(int32)]
//   pub major: i32,
//   #[prost(int32)]
//   pub minor: i32,
//   #[prost(int32)]
//   pub patch: i32,
// }
//
// /// Connect lobby request
// /// `connect` -> `lobby`
// #[derive(Debug, Message)]
// pub struct ConnectLobbyRequest {
//   pub client_version: Version,
//   pub token: String,
// }
//
// // #[derive(Debug, Copy, Clone, PartialEq, BinEncode, BinDecode)]
// // #[bin(enum_repr(u8))]
// // pub enum ConnectRejectReason {
// //   #[bin(value = 0x01)]
// //   ClientVersionTooOld,
// //   #[bin(value = 0x02)]
// //   InvalidToken,
// //   Unknown(u8),
// // }
