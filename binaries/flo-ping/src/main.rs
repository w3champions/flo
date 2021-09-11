use anyhow::Result;
use clap::Clap;
use std::collections::BTreeMap;
use std::convert::TryInto;
use std::net::{Ipv4Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::{Duration, Instant};

#[derive(Clap, Debug)]
#[clap(version = "1.0")]
struct Opts {
  address: String,
  #[clap(short)]
  number: Option<usize>,
}

fn main() -> Result<()> {
  let opts: Opts = Opts::parse();

  let address: SocketAddr = opts
    .address
    .to_socket_addrs()?
    .next()
    .expect("address not specified");
  let number = opts.number.unwrap_or(5);

  let mut map = BTreeMap::<u32, Option<Result<u32, ()>>>::new();
  while map.len() < number {
    map.insert(rand::random(), None);
  }

  let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
  socket.set_read_timeout(Duration::from_secs(1).into())?;
  socket.connect(address)?;

  let mut buf = [0_u8; 8];
  let t_base = Instant::now();

  for (i, id) in map
    .keys()
    .cloned()
    .collect::<Vec<_>>()
    .into_iter()
    .enumerate()
  {
    println!("[{}/{}] Pinging {}...", i + 1, number, address);
    buf[0..4].copy_from_slice(&id.to_be_bytes());
    buf[4..8].copy_from_slice(&((Instant::now() - t_base).as_millis() as u32).to_be_bytes());
    socket.send(&buf)?;
    loop {
      match socket.recv(&mut buf) {
        Ok(n) => {
          if n != 8 {
            continue;
          }
          let recv_id = u32::from_be_bytes(buf[0..4].try_into()?);
          if recv_id != id {
            continue;
          }
          let recv_t = u32::from_be_bytes(buf[4..8].try_into()?);
          let now = (Instant::now() - t_base).as_millis() as u32;
          let rtt = now.saturating_sub(recv_t);
          println!("time = {}ms", rtt);
          map.insert(id, Some(Ok(rtt)));
          break;
        }
        Err(err) => {
          if err.kind() == std::io::ErrorKind::WouldBlock
            || err.kind() == std::io::ErrorKind::TimedOut
          {
            println!("timeout");
            map.insert(id, Some(Err(())));
            break;
          } else {
            return Err(err.into());
          }
        }
      }
    }
    std::thread::sleep(Duration::from_secs(1));
  }

  let values: Vec<u32> = map
    .values()
    .cloned()
    .filter_map(|v| v.and_then(|v| v.ok()))
    .collect();
  let lost = number - values.len();
  let lost_rate = ((lost as f64) / number as f64 * 10000.0).round() / 100.0;

  println!("Ping statistics for {}:", address);
  println!(
    "\tPackets: Sent = {}, Received = {}, Lost = {} ({}% loss)",
    number,
    values.len(),
    number - values.len(),
    lost_rate
  );
  println!("Approximate round trip times in milli-seconds:");
  println!(
    "\tMinimum = {}, Maximum = {}, Average = {}",
    values
      .iter()
      .min()
      .map(|v| format!("{}ms", v))
      .unwrap_or("N/A".to_string()),
    values
      .iter()
      .max()
      .map(|v| format!("{}ms", v))
      .unwrap_or("N/A".to_string()),
    if values.is_empty() {
      "N/A".to_string()
    } else {
      format!("{:.2}ms", values.iter().sum::<u32>() as f64 / number as f64)
    }
  );

  Ok(())
}
