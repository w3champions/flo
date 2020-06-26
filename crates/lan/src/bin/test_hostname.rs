use std::io;

fn main() -> io::Result<()> {
  // Retrieve the hostname
  println!("{:?}", hostname::get()?);

  Ok(())
}
