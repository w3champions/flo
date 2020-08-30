pub use crate::proto::flo_node::*;

packet_type!(ControllerConnect, PacketControllerConnect);
packet_type!(ControllerConnectAccept, PacketControllerConnectAccept);
packet_type!(ControllerConnectReject, PacketControllerConnectReject);
packet_type!(ControllerUpdateSlotStatus, PacketControllerUpdateSlotStatus);
packet_type!(
  ControllerUpdateSlotStatusAccept,
  PacketControllerUpdateSlotStatusAccept
);
packet_type!(
  ControllerUpdateSlotStatusReject,
  PacketControllerUpdateSlotStatusReject
);
packet_type!(ControllerCreateGame, PacketControllerCreateGame);
packet_type!(ControllerCreateGameAccept, PacketControllerCreateGameAccept);
packet_type!(ControllerCreateGameReject, PacketControllerCreateGameReject);
packet_type!(ControllerQueryGameStatus, PacketControllerQueryGameStatus);
packet_type!(ClientConnect, PacketClientConnect);
packet_type!(ClientConnectAccept, PacketClientConnectAccept);
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
  ClientUpdateSlotClientStatusReject,
  PacketClientUpdateSlotClientStatusReject
);
packet_type!(NodeGameStatusUpdate, PacketNodeGameStatusUpdate);
packet_type!(NodeGameStatusUpdateBulk, PacketNodeGameStatusUpdateBulk);
