
use tokio::io::{split, AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::sync::{mpsc, RwLock, Mutex};
use tokio::net::{TcpStream, TcpListener};
use tokio_rustls::{TlsAcceptor, webpki, TlsConnector};
use rustls::{Certificate, PrivateKey, ServerConfig, ClientConfig, OwnedTrustAnchor};
use rustls_pemfile::{certs, rsa_private_keys};
use tokio_rustls::server::TlsStream;
use tokio_tun::{Tun, TunBuilder};

use etherparse::{SlicedPacket};
use etherparse::InternetSlice::{Ipv4, Ipv6};

use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{self, BufReader};
use std::net::{Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::sync::Arc;
use std::time::{Instant};

use crate::config;

enum AuthStatus {
    Ok,
    Failure
}

pub enum ClientState {
    Unauthorized,
    Authorized,
}

// TODO: Framing with tokio streams
// enum ClientFrame<'a> {
//     AuthRequest {
//         username: String,
//         password: String,
//     },
//     RawIp(&'a [u8]),
// }

// enum ServerFrame<'a> {
//     AuthResponse {
//         status: AuthStatus,
//         message: String,
//         inet: Ipv4Addr,
//     },
//     RawIp(&'a [u8]),
// }

enum Frame<'a> {
    AuthRequest {
        username: String,
        password: String,
    },
    AuthResponse {
        status: AuthStatus,
        message: String,
        inet: Ipv4Addr,
    },
    RawIp(&'a [u8]),
}

pub struct Server{
    // pub tunnel: Arc<Mutex<Tun>>,
    pub tunnel: Tun,
    pub port: u16,
    pub config: Arc<ServerConfig>,
    pub pool: Arc<Mutex<VecDeque<Ipv4Addr>>>,
    pub connections: Arc<RwLock<HashMap<Ipv4Addr, ClientEndpoint>>>,
}

impl Server {
    pub fn new(port: u16, tls_options: &config::TlsOptions, tun_options: &config::TunOptions) -> Result<Self, io::Error>  {
        let inet = (tun_options.tun_inet).parse::<Ipv4Addr>().unwrap();
        let tun = TunBuilder::new()
            .address(inet)
            .mtu(i32::from(tun_options.tun_mtu))
            .packet_info(false)
            .netmask(Ipv4Addr::new(255, 255, 255, 0))
            .name(&tun_options.tun_name)
            .up()
            .try_build()
            .unwrap();
        let pool = VecDeque::from_iter(
            (100..101).map(|i: u8| Ipv4Addr::new(inet.octets()[0], inet.octets()[1],inet.octets()[2], i))
        );
        let certs: Vec<Certificate> = certs(&mut BufReader::new(File::open(&tls_options.cert)?))
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid cert"))
                    .map(|mut certs| certs.drain(..).map(Certificate).collect())?;
        let key: PrivateKey = rsa_private_keys(&mut BufReader::new(File::open(&tls_options.key)?))
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid key"))
                    .map(|mut keys| keys.drain(..).map(PrivateKey).next().unwrap())?;
        let config = rustls::ServerConfig::builder()
            .with_safe_default_cipher_suites()
            .with_safe_default_kx_groups()
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .expect("bad certificate/key");
        Ok(Self { 
            tunnel: tun,
            port: port,
            pool: Arc::new(Mutex::new(pool)),
            config: Arc::new(config),
            connections: Arc::new(RwLock::new(HashMap::new()))
        })
    }
    async fn tunnel_routine(connections: Arc<RwLock<HashMap<Ipv4Addr, ClientEndpoint>>>, mut reader: ReadHalf<Tun>) -> tokio::io::Result<()> {
        let mut buf = [0u8; 1500];
        loop {
            if let Ok(n) = reader.read(&mut buf).await {
                match SlicedPacket::from_ip(&buf[0 .. n]) {
                    Err(value) => {
                        println!("Err {:?}", value);
                        break;
                    },
                    Ok(value) => {
                        match value.ip {
                            Some(Ipv4(value, _extensions)) => {
                                let dest = value.destination_addr();
                                let clients = connections.read().await;
                                if let Some(client) = clients.get(&dest) {
                                    client.writer.lock().await.write(&buf[0 .. n]).await;
                                    // if let Ok(mut stream) = Arc::try_unwrap(client.writer.clone()) {
                                    //     stream.write(&[1,2,3]);
                                    // }
                                    // let (_, mut writer) = split(stream.as_ref());
                                }
                            },
                            Some(Ipv6(_value, _extensions)) => {},
                            None => {}
                        }
                    }
                }
            }
        }

        Ok(()) as io::Result<()>
    }
    pub async fn start(self) -> tokio::io::Result<()> {                                       
        let acceptor = TlsAcceptor::from(self.config.clone());
        let (mut tunnel_reader, mut tunnel_writer) = split(self.tunnel);
        let tunnel_writer = Arc::new(Mutex::new(tunnel_writer));
        let connections = self.connections.clone();
        
        let listener = TcpListener::bind(("0.0.0.0", self.port)).await?;

        let pool = self.pool.clone();

        tokio::spawn(Server::tunnel_routine(connections.clone(), tunnel_reader));

        loop {
            let (stream, peer) = listener.accept().await?;
            let acceptor = acceptor.clone();
            // let clients = clients.clone();
            let mut stream = acceptor.accept(stream).await?;
            if let Some(client_inet) = pool.lock().await.pop_front() {
                let pool = pool.clone();
                println!("Accept client from {}: assign IP {:?}", peer, client_inet);
                let (mut reader, mut writer) = split(stream);
                let writer = Arc::new(Mutex::new(writer));
                let connections = connections.clone();
                connections.write().await.insert(client_inet, ClientEndpoint::new(writer.clone(), client_inet));
                let tunnel_writer = tunnel_writer.clone();
                let client_routine = async move {
                    let mut buf = [0u8; 1500];
                    while let Ok(n) = reader.read(&mut buf).await {
                        tunnel_writer.lock().await.write(&buf[0 .. n]).await;
                    }
                    {
                        let mut pool = pool.lock().await;
                        pool.push_back(client_inet);
                        connections.write().await.remove(&client_inet);
                        println!("Pool: {:?}", pool);
                        let existing = connections.read().await;
                        let ips: Vec<&Ipv4Addr> = existing.keys().collect();
                        println!("Clients: {:?}", ips);
                    }
                    Ok(()) as io::Result<()>
                };
                tokio::spawn(client_routine);
            } else {
                stream.shutdown();
            }
        }
                
        Ok(()) as tokio::io::Result<()>
    }
}

pub struct ClientEndpoint {
    pub state: ClientState,
    pub inet: Ipv4Addr,
    // pub peer: SocketAddr,
    pub connected_at: Instant,
    // pub stream: Arc<TlsStream<TcpStream>>,
    pub writer: Arc<Mutex<WriteHalf<TlsStream<TcpStream>>>>
    // pub tx: mpsc::Sender<Frame>,
    // pub rx: mpsc::Receiver<Frame>,
    // pub written: u64,
    // pub read: u64,
}

impl ClientEndpoint {
    pub fn new(writer: Arc<Mutex<WriteHalf<TlsStream<TcpStream>>>>, inet: Ipv4Addr) -> Self {
        ClientEndpoint {
            state: ClientState::Unauthorized,
            inet: inet,
            connected_at: Instant::now(),
            // written: 0,
            // read: 0,
            // stream: stream
            writer: writer,
        }
    }
}

pub struct Client {
    pub tunnel: Tun,
    pub domain: String,
    pub server: SocketAddr,
    pub config: Arc<ClientConfig>,
    pub state: ClientState,
    pub inet: Ipv4Addr,
    pub connected_at: Instant,
    pub writer: Option<Arc<Mutex<WriteHalf<TlsStream<TcpStream>>>>>
}

impl Client {
    pub fn new(connect: &String, username: &String, password: &String, tls_options: &config::TlsOptions, tun_options: &config::TunOptions) -> Result<Self, io::Error>  {
        let inet = Ipv4Addr::new(10, 233, 233, 100);
        let tun = TunBuilder::new()
            .address(inet)
            .mtu(i32::from(tun_options.tun_mtu))
            .packet_info(false)
            .netmask(Ipv4Addr::new(255, 255, 255, 0))
            .name(&tun_options.tun_name)
            .up()
            .try_build()
            .unwrap();
        println!("Auth: {}, {}", username, password);

        let addr = (connect.as_str()).to_socket_addrs().unwrap_or((connect.as_str(), 4508).to_socket_addrs().expect("Invalid connect string.")).next().unwrap();
        let domain = connect.split(':').next().expect("Invalid connect host.").to_string();
        let mut root_cert_store = rustls::RootCertStore::empty();
        let mut pem = BufReader::new(File::open(tls_options.cacert.as_path())?);
        let certs = rustls_pemfile::certs(&mut pem)?;
        let trust_anchors = certs.iter().map(|cert| {
            let ta = webpki::TrustAnchor::try_from_cert_der(&cert[..]).unwrap();
            OwnedTrustAnchor::from_subject_spki_name_constraints(
                ta.subject,
                ta.spki,
                ta.name_constraints,
            )
        });
        root_cert_store.add_server_trust_anchors(trust_anchors);
        
        let config = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(root_cert_store)
        .with_no_client_auth();

        Ok(Client {
            tunnel: tun,
            server: addr,
            domain: domain,
            config: Arc::new(config),
            state: ClientState::Unauthorized,
            inet: inet,
            connected_at: Instant::now(),
            writer: None
        })
    }
    pub async fn start(self) -> tokio::io::Result<()> {
        let connector = TlsConnector::from(self.config.clone());
        let stream = TcpStream::connect(&self.server).await?;

        let domain = rustls::ServerName::try_from(self.domain.as_str())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Invalid DNS name."))?;

        let mut stream = connector.connect(domain, stream).await?;

        let (mut tunnel_reader, mut tunnel_writer) = split(self.tunnel);
        let (mut reader, mut writer) = split(stream);

        // apps write to TUN ----> TLS 
        tokio::spawn(async move {
            let mut buf = [0u8; 1500];
            loop {
                if let Ok(n) = tunnel_reader.read(&mut buf).await {
                    writer.write(&buf[0 .. n]).await;
                }
            }
            Ok(()) as tokio::io::Result<()>
        });
        let mut buf = [0u8; 1500];
        loop {
            if let Ok(n) = reader.read(&mut buf).await {
                tunnel_writer.write(&buf[0 .. n]).await;
            }
        }
        Ok(()) as tokio::io::Result<()>
    }
}
