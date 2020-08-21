pub use crate::proto::flo_node::*;

packet_type!(ControllerConnect, PacketControllerConnect);
packet_type!(ControllerConnectAccept, PacketControllerConnectAccept);
packet_type!(ControllerConnectReject, PacketControllerConnectReject);
packet_type!(ControllerCreateGame, PacketControllerCreateGame);
packet_type!(ControllerCreateGameAccept, PacketControllerCreateGameAccept);
packet_type!(ControllerCreateGameReject, PacketControllerCreateGameReject);
packet_type!(ClientConnect, PacketClientConnect);
packet_type!(ClientConnectReject, PacketClientConnectReject);
packet_type!(
  ClientUpdateSlotClientStatusRequest,
  PacketClientUpdateSlotClientStatusRequest
);
packet_type!(
  ClientUpdateSlotClientStatus,
  PacketClientUpdateSlotClientStatus
);
packet_type!(
  ClientUpdateSlotClientStatus,
  PacketClientUpdateSlotClientStatusReject
);
packet_type!(NodeGameStatusUpdate, PacketNodeGameStatusUpdate);
