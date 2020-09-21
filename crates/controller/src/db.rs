

pub use bs_diesel_utils::{lock::transaction_with_advisory_lock, DbConn, Executor, ExecutorRef};


// #[repr(u8)]
// #[derive(Clone, Copy)]
// enum LockTypeId {
//   Game = 1,
// }
//
// pub(crate) struct DbLockId {
//   ty: LockTypeId,
//   data: [u8; 7],
// }
//
// impl DbLockId {
//   pub fn game(id: i32) -> Self {
//     DbLockId::new_i32(LockTypeId::Game, id)
//   }
//
//   fn new_i32(ty: LockTypeId, value: i32) -> Self {
//     let mut data: [u8; 7] = [0; 7];
//     data.copy_from_slice(&value.to_le_bytes());
//     Self { ty, data }
//   }
// }
//
// impl AsAdvisoryLockID for DbLockId {
//   fn as_advisory_lock_id(&self) -> i64 {
//     let mut data: [u8; 8] = [0; 8];
//     data.copy_from_slice(&self.data);
//     data[7] = self.ty as u8;
//     i64::from_le_bytes(data)
//   }
// }
