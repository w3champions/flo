pub trait BroadcastTarget {
  fn contains(&self, player_id: i32) -> bool;
}

pub struct Everyone;
impl BroadcastTarget for Everyone {
  fn contains(&self, _player_id: i32) -> bool {
    true
  }
}

pub struct AllowList<'a>(pub &'a [i32]);
impl<'a> BroadcastTarget for AllowList<'a> {
  fn contains(&self, player_id: i32) -> bool {
    self.0.contains(&player_id)
  }
}

pub struct DenyList<'a>(pub &'a [i32]);
impl<'a> BroadcastTarget for DenyList<'a> {
  fn contains(&self, player_id: i32) -> bool {
    !self.0.contains(&player_id)
  }
}

pub struct Filter<F>(F);
impl<F> BroadcastTarget for Filter<F>
where
  F: Fn(i32) -> bool,
{
  fn contains(&self, player_id: i32) -> bool {
    (self.0)(player_id)
  }
}
