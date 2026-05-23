use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::thread;

use persona::engine::EngineComponent;
use persona::transport::{ComponentHandoffEndpoint, ComponentHandoffRouter};
use persona::upgrade::Version;
use unix_ancillary::UnixStreamExt;

fn unique_root(label: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_nanos();
    PathBuf::from("/tmp").join(format!(
        "persona-handoff-{}-{label}-{unique}",
        std::process::id()
    ))
}

fn spawn_receiver(
    control_socket_path: PathBuf,
    expected_request: Vec<u8>,
    reply: Vec<u8>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let control =
            UnixStream::connect(control_socket_path).expect("component connects control socket");
        let received = control
            .recv_fds::<1>()
            .expect("component receives handed-off descriptor");
        let file_descriptor = received
            .fds
            .into_iter()
            .next()
            .expect("handoff carried one public client descriptor");
        let mut stream = UnixStream::from(file_descriptor);
        let mut request = vec![0_u8; expected_request.len()];
        stream
            .read_exact(&mut request)
            .expect("component reads client request");
        assert_eq!(request, expected_request);
        stream
            .write_all(&reply)
            .expect("component writes client reply");
        stream.flush().expect("component flushes client reply");
    })
}

fn spawn_client(
    public_socket_path: PathBuf,
    request: Vec<u8>,
    expected_reply: Vec<u8>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut stream =
            UnixStream::connect(public_socket_path).expect("client connects public socket");
        stream.write_all(&request).expect("client writes request");
        stream.flush().expect("client flushes request");
        let mut reply = vec![0_u8; expected_reply.len()];
        stream.read_exact(&mut reply).expect("client reads reply");
        assert_eq!(reply, expected_reply);
    })
}

#[tokio::test]
async fn constraint_persona_handoff_router_binds_public_and_control_sockets_with_boundary_modes() {
    let root = unique_root("modes");
    let public_socket_path = root.join("message.sock");
    let control_socket_path = root.join("control").join("message.sock");
    let endpoint = ComponentHandoffEndpoint::new(
        EngineComponent::Message,
        &public_socket_path,
        &control_socket_path,
    );

    let router = ComponentHandoffRouter::bind(endpoint).expect("handoff router binds");

    assert_eq!(
        router.endpoint().public_socket_path(),
        public_socket_path.as_path()
    );
    assert_eq!(
        router.endpoint().control_socket_path(),
        control_socket_path.as_path()
    );
    let public_mode = std::fs::metadata(&public_socket_path)
        .expect("public socket metadata")
        .permissions()
        .mode()
        & 0o777;
    let control_mode = std::fs::metadata(&control_socket_path)
        .expect("control socket metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        public_mode,
        EngineComponent::Message.socket_mode().as_octal()
    );
    assert_eq!(control_mode, 0o600);
}

#[tokio::test]
async fn constraint_persona_handoff_router_selects_active_version_per_public_connection() {
    let root = unique_root("active-version");
    let public_socket_path = root.join("message.sock");
    let control_socket_path = root.join("control").join("message.sock");
    let endpoint = ComponentHandoffEndpoint::new(
        EngineComponent::Message,
        &public_socket_path,
        &control_socket_path,
    );
    let mut router = ComponentHandoffRouter::bind(endpoint).expect("handoff router binds");
    let main_version = Version::new("v0.1.0");
    let next_version = Version::new("v0.1.1");

    let main_receiver = spawn_receiver(
        control_socket_path.clone(),
        b"main-request".to_vec(),
        b"main-reply".to_vec(),
    );
    router
        .accept_receiver_for_version(main_version.clone())
        .await
        .expect("main receiver registers");
    let next_receiver = spawn_receiver(
        control_socket_path.clone(),
        b"next-request".to_vec(),
        b"next-reply".to_vec(),
    );
    router
        .accept_receiver_for_version(next_version.clone())
        .await
        .expect("next receiver registers");

    let main_client = spawn_client(
        public_socket_path.clone(),
        b"main-request".to_vec(),
        b"main-reply".to_vec(),
    );
    router
        .handoff_one(&main_version)
        .await
        .expect("main active version receives public connection");
    main_client.join().expect("main client completes");
    main_receiver.join().expect("main receiver completes");

    let next_client = spawn_client(
        public_socket_path,
        b"next-request".to_vec(),
        b"next-reply".to_vec(),
    );
    router
        .handoff_one(&next_version)
        .await
        .expect("next active version receives public connection");
    next_client.join().expect("next client completes");
    next_receiver.join().expect("next receiver completes");
}
