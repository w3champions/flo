Prepare environment
-------------------

Update packages

```shell
apt update
apt upgrade
```

Install required packages

```shell
apt install build-essential cmake postgresql git libpq-dev
```

Install rustup

```shell
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.bashrc
```

Get code
--------

```shell
git clone --recursive https://github.com/w3champions/flo.git
```

Prepare database
----------------

install diesel_cli

```shell
cd flo
export PQ_LIB_DIR=/usr/lib/x86_64-linux-gnu/
cargo install diesel_cli --no-default-features --features postgres
```

enable password auth instead of peer by editing `/etc/postgresql/11/main/pg_hba.conf`

should be ```local  all  postgres  md5```

restart postgres

```shell
systemctl restart postgresql
```

set password for pgsql user

```bash
su postgres
pgsql
postgres=# \password
Enter new password: 
Enter it again: 
postgres=# exit
exit
```

create `.env` file in flo code root
```ini
RUST_LOG=debug
DATABASE_URL=postgres://postgres:postgres@localhost/flo
JWT_SECRET_BASE64=dGVzdHRlc3R0ZXN0dGVzdHRlc3R0ZXN0dGVzdHRlc3R0ZXN0dGVzdHRlc3Q=
```

as pgsql user create database and fill it using diesel
(use your password)

```shell
export PGPASSWORD=postgres
psql -U postgres -c "create database flo"
diesel setup
```

add api_client and node rows to postgres

```shell
psql -U postgres -d flo -c "insert into api_client (name, secret_key) VALUES ('mawa', 'mawa')"
psql -U postgres -d flo -c "insert into node (name, location, secret, ip_addr) VALUES ('mawa', 'US 6', 'mawa', '127.0.0.1')"
```

Building
--------

build controller and node services

```shell
cargo build -p flo-controller-service --release
cargo build -p flo-node-service --release
```

Running
-------

export secret key

```shell
export FLO_NODE_SECRET='mawa'
```

run node first

```shell
./target/release/flo-node-service
./target/release/flo-controller-service
```

TODO
----

Actually need to write systemd `.service` files for those, to use:

```shell
systemctl start flo-node-service
systemctl start flo-controller-service
```

Trace logs:

```shell
journalctl -f -u flo-node-service
```
