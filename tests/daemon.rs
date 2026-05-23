use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nota_codec::{Encoder, NotaEncode};
use owner_signal_version_handover::{
    AttemptHandover, ForceFlip, ForceReason, Operation as OwnerOperation, Quarantine,
    QuarantineReason, RejectionReason, Reply as OwnerReply, SocketPath, Version as OwnerVersion,
    VersionEndpoint, VersionLabel,
};
use persona::engine::{EngineComponent, EngineTopology};
use persona::engine_event::EngineEventBody;
use persona::transport::{OwnerClient, OwnerEndpoint, PersonaDaemon, PersonaEndpoint};
use persona::unit::{ComponentUnit, UnitAction, UnitController, UnitFuture, UnitReceipt};
use persona::upgrade::{HandoverClient, HandoverEndpoint, HandoverFrameCodec};
use persona_spirit::{
    DaemonConfiguration as SpiritDaemonConfiguration, DaemonRuntime as SpiritDaemonRuntime,
    SocketMode as SpiritSocketMode, SocketPath as SpiritSocketPath, StorePath as SpiritStorePath,
    ordinary as spirit_ordinary,
};
use signal_persona_spirit::{
    Context, Entry, Kind, Observation, ObservationMode, Operation as SpiritOperation, Quote,
    RecordAccepted, RecordIdentifier, RecordQuery, RecordSummary, RecordsObserved,
    Reply as SpiritReply, Summary, Topic,
};
use signal_sema::Magnitude;
use signal_version_handover::{
    Date, HandoverAcceptance, HandoverFinalization, HandoverMarker, HandoverRejectionReason,
    Operation as HandoverOperation, RecoveryRequest, Reply as HandoverReply, Time,
};
use version_projection::{ComponentName as HandoverComponentName, ContractVersion};

mod support;

struct DaemonFixture {
    root: PathBuf,
    socket: PathBuf,
    manager_store: PathBuf,
    daemon: Child,
}

struct SpiritUpgradeSocketFixture {
    root: PathBuf,
    ordinary_socket: SpiritSocketPath,
    owner_socket: SpiritSocketPath,
    upgrade_socket: SpiritSocketPath,
    store: SpiritStorePath,
}

struct ExternalSpiritDaemon {
    child: Child,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecordedUnitAction {
    action: UnitAction,
    unit: ComponentUnit,
}

#[derive(Debug, Clone, Default)]
struct RecordingUnitController {
    actions: Arc<Mutex<Vec<RecordedUnitAction>>>,
}

impl RecordingUnitController {
    fn record(&self, unit: ComponentUnit, action: UnitAction) -> UnitReceipt {
        self.actions
            .lock()
            .expect("unit action log lock")
            .push(RecordedUnitAction {
                action,
                unit: unit.clone(),
            });
        UnitReceipt::from_action(unit, action)
    }

    fn actions(&self) -> Vec<RecordedUnitAction> {
        self.actions.lock().expect("unit action log lock").clone()
    }
}

impl UnitController for RecordingUnitController {
    fn start<'a>(&'a self, unit: ComponentUnit) -> UnitFuture<'a, UnitReceipt> {
        Box::pin(async move { Ok(self.record(unit, UnitAction::Start)) })
    }

    fn stop<'a>(&'a self, unit: ComponentUnit) -> UnitFuture<'a, UnitReceipt> {
        Box::pin(async move { Ok(self.record(unit, UnitAction::Stop)) })
    }

    fn restart<'a>(&'a self, unit: ComponentUnit) -> UnitFuture<'a, UnitReceipt> {
        Box::pin(async move { Ok(self.record(unit, UnitAction::Restart)) })
    }

    fn status<'a>(
        &'a self,
        unit: ComponentUnit,
    ) -> UnitFuture<'a, persona::unit::UnitStatusReport> {
        Box::pin(async move {
            Ok(persona::unit::UnitStatusReport::new(
                unit,
                persona::unit::UnitStatus::Active,
            ))
        })
    }
}

impl DaemonFixture {
    fn start() -> Self {
        let root = Self::unique_root();
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("test root created");
        let socket = root.join("persona.sock");
        let manager_store = root.join("manager.redb");
        let mut daemon = Command::new(env!("CARGO_BIN_EXE_persona-daemon"))
            .arg(&socket)
            .env("PERSONA_MANAGER_STORE", &manager_store)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("persona-daemon starts");

        let stdout = daemon.stdout.take().expect("daemon stdout is piped");
        let mut reader = BufReader::new(stdout);
        let mut readiness = String::new();
        reader
            .read_line(&mut readiness)
            .expect("daemon reports readiness");

        assert_eq!(
            readiness.trim(),
            format!("persona-daemon socket={}", socket.display())
        );

        Self {
            root,
            socket,
            manager_store,
            daemon,
        }
    }

    fn start_with_prototype_supervised_components() -> Self {
        Self::start_with_engine_topology(EngineTopology::FullPrototype)
    }

    fn start_with_message_router_components() -> Self {
        Self::start_with_engine_topology(EngineTopology::MessageRouter)
    }

    fn start_with_three_harness_chain_components() -> Self {
        Self::start_with_engine_topology(EngineTopology::ThreeHarnessChain)
    }

    fn start_with_engine_topology(topology: EngineTopology) -> Self {
        let root = Self::unique_root();
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("test root created");
        let socket = root.join("persona.sock");
        let manager_store = root.join("manager.redb");
        let script = support::component_socket_fixture(root.as_path());
        let mut daemon = Command::new(env!("CARGO_BIN_EXE_persona-daemon"))
            .arg(&socket)
            .env("PERSONA_MANAGER_STORE", &manager_store)
            .env("PERSONA_STATE_ROOT", root.join("state"))
            .env("PERSONA_RUN_ROOT", root.join("run"))
            .env("PERSONA_PROTOTYPE_STACK_EXECUTABLE", script)
            .env("PERSONA_ENGINE_TOPOLOGY", topology.as_str())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("persona-daemon starts");

        let stdout = daemon.stdout.take().expect("daemon stdout is piped");
        let mut reader = BufReader::new(stdout);
        let mut readiness = String::new();
        reader
            .read_line(&mut readiness)
            .expect("daemon reports readiness");

        assert_eq!(
            readiness.trim(),
            format!("persona-daemon socket={}", socket.display())
        );

        Self {
            root,
            socket,
            manager_store,
            daemon,
        }
    }

    fn unique_root() -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        PathBuf::from("/tmp").join(format!("p-daemon-{}-{unique}", std::process::id()))
    }

    fn stop_daemon(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
        self.stop_component_process_groups();
    }

    fn persona(&self, request: &str) -> String {
        let output = Command::new(env!("CARGO_BIN_EXE_persona"))
            .arg(request)
            .env("PERSONA_SOCKET", &self.socket)
            .output()
            .expect("persona client runs");

        assert!(
            output.status.success(),
            "persona failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        String::from_utf8(output.stdout).expect("persona output is utf8")
    }

    fn owner_client(&self) -> OwnerClient {
        OwnerClient::new(OwnerEndpoint::from_path(
            self.root.join("persona-owner.sock"),
        ))
    }

    fn component_capture(&self, component: EngineComponent) -> PathBuf {
        self.root
            .join("state")
            .join("default")
            .join(format!("{}.env", component.as_str()))
    }

    fn component_instance_capture(&self, instance_name: &str) -> PathBuf {
        self.root
            .join("state")
            .join("default")
            .join(format!("{instance_name}.env"))
    }

    fn wait_for_component_capture(&self, component: EngineComponent) -> String {
        let path = self.component_capture(component);
        for _attempt in 0..80 {
            if let Ok(text) = std::fs::read_to_string(&path)
                && text.contains("peer_count=")
            {
                return text;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        panic!("component capture did not appear: {}", path.display());
    }

    fn stop_component_process_groups(&self) {
        let capture_dir = self.root.join("state").join("default");
        let Ok(entries) = std::fs::read_dir(capture_dir) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(text) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            let Some(process) = text.lines().find_map(|line| {
                line.strip_prefix("process=")
                    .and_then(|value| value.parse::<i32>().ok())
            }) else {
                continue;
            };
            unsafe {
                libc::killpg(process, libc::SIGTERM);
                libc::killpg(process, libc::SIGKILL);
            }
        }
    }
}

impl SpiritUpgradeSocketFixture {
    fn new(name: &str) -> Self {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        let root = PathBuf::from("/tmp").join(format!(
            "persona-spirit-{name}-{}-{unique}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("spirit upgrade fixture root created");
        Self {
            ordinary_socket: SpiritSocketPath::new(
                root.join("spirit.sock").to_string_lossy().into_owned(),
            ),
            owner_socket: SpiritSocketPath::new(
                root.join("spirit-owner.sock")
                    .to_string_lossy()
                    .into_owned(),
            ),
            upgrade_socket: SpiritSocketPath::new(
                root.join("spirit-upgrade.sock")
                    .to_string_lossy()
                    .into_owned(),
            ),
            store: SpiritStorePath::new(
                root.join("persona-spirit.redb")
                    .to_string_lossy()
                    .into_owned(),
            ),
            root,
        }
    }

    fn configuration(&self) -> SpiritDaemonConfiguration {
        SpiritDaemonConfiguration::new(
            self.ordinary_socket.clone(),
            self.owner_socket.clone(),
            self.upgrade_socket.clone(),
            self.store.clone(),
            SpiritSocketMode::from_octal(0o600),
        )
    }

    fn upgrade_socket_path(&self) -> &std::path::Path {
        self.upgrade_socket.as_path()
    }

    fn ordinary_socket_path(&self) -> &std::path::Path {
        self.ordinary_socket.as_path()
    }

    fn store_path(&self) -> &std::path::Path {
        self.store.as_path()
    }

    fn client(&self) -> spirit_ordinary::SignalClient {
        spirit_ordinary::SignalClient::new(self.ordinary_socket.clone())
    }

    fn copy_store_from(&self, source: &Self) {
        std::fs::copy(source.store_path(), self.store_path()).expect("spirit store copy succeeds");
    }
}

impl ExternalSpiritDaemon {
    fn spawn(binary: &std::path::Path, configuration: &SpiritDaemonConfiguration) -> Self {
        let mut encoder = Encoder::new();
        configuration
            .encode(&mut encoder)
            .expect("spirit daemon configuration encodes");
        let configuration_text = encoder.into_string();
        let child = Command::new(binary)
            .arg(configuration_text)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|error| {
                panic!(
                    "spawn external persona-spirit-daemon {}: {error}",
                    binary.display()
                )
            });
        Self { child }
    }

    fn assert_running(&mut self, socket: &std::path::Path) {
        if let Some(status) = self
            .child
            .try_wait()
            .expect("query external spirit daemon status")
        {
            panic!(
                "external persona-spirit-daemon exited before {} appeared: status={status}, output={}",
                socket.display(),
                self.output()
            );
        }
    }

    fn output(&mut self) -> String {
        let mut output = String::new();
        if let Some(mut stdout) = self.child.stdout.take() {
            let mut text = String::new();
            let _ = stdout.read_to_string(&mut text);
            if !text.is_empty() {
                output.push_str("stdout=");
                output.push_str(&text);
            }
        }
        if let Some(mut stderr) = self.child.stderr.take() {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            if !text.is_empty() {
                if !output.is_empty() {
                    output.push(' ');
                }
                output.push_str("stderr=");
                output.push_str(&text);
            }
        }
        output
    }
}

impl Drop for ExternalSpiritDaemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spirit_entry(summary: &str) -> Entry {
    Entry {
        topic: Topic::new("workspace"),
        kind: Kind::Decision,
        summary: Summary::new(summary),
        context: Context::new("copied handover witness"),
        certainty: Magnitude::Maximum,
        quote: Quote::new("persona copied handover witness"),
    }
}

fn observe_spirit_summaries() -> SpiritOperation {
    SpiritOperation::Observe(Observation::Records(RecordQuery {
        topic: None,
        kind: None,
        mode: ObservationMode::SummaryOnly,
    }))
}

fn owner_version(label: &str, byte: u8) -> OwnerVersion {
    OwnerVersion::new(VersionLabel::new(label), ContractVersion::new([byte; 32]))
}

fn handover_marker(commit_sequence: u64) -> HandoverMarker {
    HandoverMarker {
        component: HandoverComponentName::new("persona-spirit"),
        schema_hash: ContractVersion::new([9; 32]),
        commit_sequence,
        write_counter: 3,
        last_record_identifier: Some(210),
        recorded_at_date: Date::new(2026, 5, 22),
        recorded_at_time: Time::new(17, 30, 0),
    }
}

fn owner_version_endpoint(
    label: &str,
    byte: u8,
    upgrade_socket_path: &std::path::Path,
) -> VersionEndpoint {
    let root = format!("/run/persona/default/spirit/{label}");
    VersionEndpoint {
        version: owner_version(label, byte),
        owner_socket_path: SocketPath::new(format!("{root}/owner.sock")),
        upgrade_socket_path: SocketPath::new(upgrade_socket_path.to_string_lossy().into_owned()),
    }
}

fn owner_attempt_handover_order_with_upgrade_sockets(
    current_upgrade_socket_path: &std::path::Path,
    next_upgrade_socket_path: &std::path::Path,
) -> AttemptHandover {
    AttemptHandover {
        component: HandoverComponentName::new("persona-spirit"),
        current: owner_version_endpoint("v0.1.0", 1, current_upgrade_socket_path),
        next: owner_version_endpoint("v0.1.1", 2, next_upgrade_socket_path),
    }
}

fn owner_quarantine_order() -> Quarantine {
    Quarantine {
        component: HandoverComponentName::new("persona-spirit"),
        version: owner_version("v0.1.1", 2),
        reason: QuarantineReason::SuspectState,
    }
}

fn owner_force_current_order() -> ForceFlip {
    ForceFlip {
        component: HandoverComponentName::new("persona-spirit"),
        current_version: owner_version("v0.1.0", 1),
        target_version: owner_version("v0.1.0", 1),
        reason: ForceReason::OperatorOverride,
    }
}

async fn serve_current_handover_socket(
    path: std::path::PathBuf,
    marker: HandoverMarker,
) -> persona::Result<Vec<HandoverOperation>> {
    let listener = tokio::net::UnixListener::bind(path)?;
    let codec = HandoverFrameCodec::default();
    let mut operations = Vec::new();
    for _ in 0..3 {
        let (mut stream, _) = listener.accept().await?;
        let frame = codec.read_frame(&mut stream).await?;
        let received = codec.request_from_frame(frame)?;
        let exchange = received.exchange();
        let operation = received.into_operation();
        let reply = match &operation {
            HandoverOperation::AskHandoverMarker(request) => {
                assert_eq!(request.component.as_str(), "persona-spirit");
                HandoverReply::HandoverMarker(marker.clone())
            }
            HandoverOperation::ReadyToHandover(report) => {
                assert_eq!(report.component.as_str(), "persona-spirit");
                assert_eq!(report.source_marker.commit_sequence, marker.commit_sequence);
                HandoverReply::HandoverAccepted(HandoverAcceptance {
                    accepted_marker: marker.clone(),
                })
            }
            HandoverOperation::HandoverCompleted(report) => {
                assert_eq!(report.component.as_str(), "persona-spirit");
                assert_eq!(
                    report.accepted_marker.commit_sequence,
                    marker.commit_sequence
                );
                HandoverReply::HandoverFinalized(HandoverFinalization {
                    finalized_marker: marker.clone(),
                })
            }
            other => panic!("unexpected handover operation in test server: {other:?}"),
        };
        let frame = codec.reply_frame(exchange, reply);
        codec.write_frame(&mut stream, &frame).await?;
        operations.push(operation);
    }
    Ok(operations)
}

async fn serve_marker_handover_socket(
    path: std::path::PathBuf,
    marker: HandoverMarker,
) -> persona::Result<Vec<HandoverOperation>> {
    let listener = tokio::net::UnixListener::bind(path)?;
    let codec = HandoverFrameCodec::default();
    let (mut stream, _) = listener.accept().await?;
    let frame = codec.read_frame(&mut stream).await?;
    let received = codec.request_from_frame(frame)?;
    let exchange = received.exchange();
    let operation = received.into_operation();
    let reply = match &operation {
        HandoverOperation::AskHandoverMarker(request) => {
            assert_eq!(request.component.as_str(), "persona-spirit");
            HandoverReply::HandoverMarker(marker)
        }
        other => panic!("unexpected next handover operation in test server: {other:?}"),
    };
    let frame = codec.reply_frame(exchange, reply);
    codec.write_frame(&mut stream, &frame).await?;
    Ok(vec![operation])
}

async fn serve_recovering_current_proxy(
    proxy_path: std::path::PathBuf,
    real_path: std::path::PathBuf,
) -> persona::Result<Vec<HandoverOperation>> {
    let listener = tokio::net::UnixListener::bind(proxy_path)?;
    let codec = HandoverFrameCodec::default();
    let real = HandoverClient::new(HandoverEndpoint::from_path(real_path));
    let mut operations = Vec::new();

    for _ in 0..4 {
        let (mut stream, _) = listener.accept().await?;
        let frame = codec.read_frame(&mut stream).await?;
        let received = codec.request_from_frame(frame)?;
        let exchange = received.exchange();
        let operation = received.into_operation();
        let reply = real.submit(operation.clone()).await?;

        match (&operation, &reply) {
            (
                HandoverOperation::ReadyToHandover(report),
                HandoverReply::HandoverAccepted(acceptance),
            ) => {
                let recovery = real
                    .submit(HandoverOperation::RecoverFromFailure(RecoveryRequest {
                        component: report.component.clone(),
                        failure_identifier: acceptance.accepted_marker.commit_sequence,
                    }))
                    .await?;
                match recovery {
                    HandoverReply::RecoveryCompleted(result) => {
                        assert!(result.recovered);
                    }
                    other => panic!("expected proxy recovery completion, got {other:?}"),
                }
            }
            (
                HandoverOperation::HandoverCompleted(_),
                HandoverReply::HandoverRejected(rejection),
            ) => {
                assert_eq!(rejection.reason, HandoverRejectionReason::NotReady);
            }
            (
                HandoverOperation::RecoverFromFailure(_),
                HandoverReply::RecoveryCompleted(result),
            ) => {
                assert!(result.recovered);
            }
            _ => {}
        }

        let frame = codec.reply_frame(exchange, reply);
        codec.write_frame(&mut stream, &frame).await?;
        operations.push(operation);
    }

    Ok(operations)
}

async fn wait_for_socket(path: &std::path::Path) {
    for _attempt in 0..80 {
        if path.exists() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("socket did not appear: {}", path.display());
}

async fn wait_for_external_spirit_socket(
    daemon: &mut ExternalSpiritDaemon,
    path: &std::path::Path,
) {
    for _attempt in 0..80 {
        if path.exists() {
            return;
        }
        daemon.assert_running(path);
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    daemon.assert_running(path);
    panic!("external spirit socket did not appear: {}", path.display());
}

fn external_spirit_daemon_binary() -> Option<PathBuf> {
    match std::env::var_os("PERSONA_SPIRIT_DAEMON_BIN") {
        Some(path) => Some(PathBuf::from(path)),
        None if std::env::var_os("PERSONA_REQUIRE_EXTERNAL_SPIRIT_DAEMON").is_some() => {
            panic!(
                "PERSONA_SPIRIT_DAEMON_BIN must be set for the real Spirit daemon binary witness"
            )
        }
        None => None,
    }
}

impl Drop for DaemonFixture {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
        self.stop_component_process_groups();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Drop for SpiritUpgradeSocketFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_persona_daemon_launches_three_harness_chain_topology_through_engine_supervisor()
{
    let mut fixture = DaemonFixture::start_with_three_harness_chain_components();
    let topology = EngineTopology::ThreeHarnessChain;

    for (instance_name, component, peer_count) in [
        ("message", EngineComponent::Message, 7),
        ("router", EngineComponent::Router, 7),
        ("initiator-terminal", EngineComponent::Terminal, 7),
        ("initiator", EngineComponent::Harness, 7),
        ("responder-terminal", EngineComponent::Terminal, 7),
        ("responder", EngineComponent::Harness, 7),
        ("reviewer-terminal", EngineComponent::Terminal, 7),
        ("reviewer", EngineComponent::Harness, 7),
    ] {
        let path = fixture.component_instance_capture(instance_name);
        let mut capture = String::new();
        for _attempt in 0..80 {
            if let Ok(text) = std::fs::read_to_string(&path)
                && text.contains("peer_count=")
            {
                capture = text;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(
            !capture.is_empty(),
            "component instance capture did not appear: {}",
            path.display()
        );
        assert!(capture.contains("engine=default"));
        assert!(capture.contains(&format!("component={}", component.as_str())));
        assert!(capture.contains(&format!("component_instance={instance_name}")));
        assert!(capture.contains(&format!("peer_count={peer_count}")));
    }

    fixture.stop_daemon();

    let store = persona::manager_store::ManagerStore::start(
        persona::manager_store::ManagerStoreLocation::new(&fixture.manager_store),
    )
    .expect("manager store starts for inspection");
    let events = store
        .ask(persona::manager_store::ReadEngineEvents::new(
            signal_persona_auth::EngineId::new("default"),
        ))
        .await
        .expect("default engine events read through manager store actor");
    assert_eq!(
        events.len(),
        topology.component_topology_entries().len() * 2
    );

    store.stop_gracefully().await.expect("manager store stops");
    let _shutdown_completion = store.wait_for_shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_persona_daemon_serves_owner_version_handover_socket() {
    let fixture = DaemonFixture::start();
    let reply = fixture
        .owner_client()
        .submit(OwnerOperation::Quarantine(owner_quarantine_order()))
        .await
        .expect("owner version handover request succeeds");

    match reply {
        OwnerReply::Quarantined(quarantined) => {
            assert_eq!(quarantined.component.as_str(), "persona-spirit");
            assert_eq!(quarantined.version.label.as_str(), "v0.1.1");
        }
        other => panic!("expected quarantine reply, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_persona_daemon_owner_socket_drives_version_handover() {
    let mut fixture = DaemonFixture::start();
    let current_upgrade_socket = fixture.root.join("spirit-current-upgrade.sock");
    let next_upgrade_socket = fixture.root.join("spirit-next-upgrade.sock");
    let marker = handover_marker(233);
    let server = tokio::spawn(serve_current_handover_socket(
        current_upgrade_socket.clone(),
        marker.clone(),
    ));
    let next_server = tokio::spawn(serve_marker_handover_socket(
        next_upgrade_socket.clone(),
        marker.clone(),
    ));
    wait_for_socket(&current_upgrade_socket).await;
    wait_for_socket(&next_upgrade_socket).await;

    let reply = fixture
        .owner_client()
        .submit(OwnerOperation::AttemptHandover(
            owner_attempt_handover_order_with_upgrade_sockets(
                &current_upgrade_socket,
                &next_upgrade_socket,
            ),
        ))
        .await
        .expect("owner attempt handover request succeeds");

    match reply {
        OwnerReply::HandoverSucceeded(success) => {
            assert_eq!(success.component.as_str(), "persona-spirit");
            assert_eq!(success.active_version.label.as_str(), "v0.1.1");
            assert_eq!(success.commit_sequence, 233);
        }
        other => panic!("expected handover success reply, got {other:?}"),
    }

    let operations = server
        .await
        .expect("handover socket server joins")
        .expect("handover socket server succeeds");
    assert!(matches!(
        operations.as_slice(),
        [
            HandoverOperation::AskHandoverMarker(_),
            HandoverOperation::ReadyToHandover(_),
            HandoverOperation::HandoverCompleted(_)
        ]
    ));
    let next_operations = next_server
        .await
        .expect("next handover marker socket server joins")
        .expect("next handover marker socket server succeeds");
    assert!(matches!(
        next_operations.as_slice(),
        [HandoverOperation::AskHandoverMarker(_)]
    ));

    fixture.stop_daemon();

    let store = persona::manager_store::ManagerStore::start(
        persona::manager_store::ManagerStoreLocation::new(&fixture.manager_store),
    )
    .expect("manager store starts for inspection");
    let active = store
        .ask(persona::manager_store::ReadActiveVersion::new(
            signal_persona_auth::EngineId::new("default"),
            signal_persona::ComponentName::new("persona-spirit"),
        ))
        .await
        .expect("active version read")
        .expect("active version persisted");
    assert_eq!(active.active_version().as_str(), "v0.1.1");
    assert_eq!(active.commit_sequence(), Some(233));

    store.stop_gracefully().await.expect("manager store stops");
    let _shutdown_completion = store.wait_for_shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_persona_daemon_owner_handover_uses_injected_unit_controller() {
    let root = DaemonFixture::unique_root();
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("test root created");
    let socket = root.join("persona.sock");
    let owner_socket = root.join("persona-owner.sock");
    let manager_store = root.join("manager.redb");
    let controller = RecordingUnitController::default();
    let daemon = PersonaDaemon::with_manager_store_and_owner_endpoint(
        PersonaEndpoint::from_path(&socket),
        OwnerEndpoint::from_path(&owner_socket),
        persona::manager_store::ManagerStoreLocation::new(&manager_store),
    )
    .with_unit_controller(Arc::new(controller.clone()));
    let daemon_task = tokio::spawn(async move { daemon.serve().await });
    wait_for_socket(&socket).await;
    wait_for_socket(&owner_socket).await;

    let current_upgrade_socket = root.join("spirit-current-upgrade.sock");
    let next_upgrade_socket = root.join("spirit-next-upgrade.sock");
    let marker = handover_marker(377);
    let current_server = tokio::spawn(serve_current_handover_socket(
        current_upgrade_socket.clone(),
        marker.clone(),
    ));
    let next_server = tokio::spawn(serve_marker_handover_socket(
        next_upgrade_socket.clone(),
        marker.clone(),
    ));
    wait_for_socket(&current_upgrade_socket).await;
    wait_for_socket(&next_upgrade_socket).await;

    let reply = OwnerClient::new(OwnerEndpoint::from_path(&owner_socket))
        .submit(OwnerOperation::AttemptHandover(
            owner_attempt_handover_order_with_upgrade_sockets(
                &current_upgrade_socket,
                &next_upgrade_socket,
            ),
        ))
        .await
        .expect("owner handover reaches in-process daemon");
    match reply {
        OwnerReply::HandoverSucceeded(success) => {
            assert_eq!(success.active_version.label.as_str(), "v0.1.1");
            assert_eq!(success.commit_sequence, 377);
        }
        other => panic!("expected handover success reply, got {other:?}"),
    }

    let current_operations = current_server
        .await
        .expect("current handover server joins")
        .expect("current handover server succeeds");
    assert_eq!(current_operations.len(), 3);
    let next_operations = next_server
        .await
        .expect("next handover server joins")
        .expect("next handover server succeeds");
    assert_eq!(next_operations.len(), 1);

    let actions = controller.actions();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].action, UnitAction::Start);
    assert_eq!(
        actions[0].unit.name().as_str(),
        "persona-component@persona-spirit:v0.1.1.service"
    );

    daemon_task.abort();
    let _ = daemon_task.await;
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_persona_daemon_owner_socket_drives_real_spirit_upgrade_socket() {
    let current_spirit = SpiritUpgradeSocketFixture::new("real-current-upgrade-socket");
    let next_spirit = SpiritUpgradeSocketFixture::new("real-next-upgrade-socket");
    let current_configuration = current_spirit.configuration();
    let next_configuration = next_spirit.configuration();
    let current_thread = std::thread::spawn(move || {
        SpiritDaemonRuntime::from_configuration(current_configuration)
            .bind()?
            .serve_upgrade_count(3)
    });
    let next_thread = std::thread::spawn(move || {
        SpiritDaemonRuntime::from_configuration(next_configuration)
            .bind()?
            .serve_upgrade_count(1)
    });
    wait_for_socket(current_spirit.upgrade_socket_path()).await;
    wait_for_socket(next_spirit.upgrade_socket_path()).await;

    let mut persona = DaemonFixture::start();
    let reply = persona
        .owner_client()
        .submit(OwnerOperation::AttemptHandover(
            owner_attempt_handover_order_with_upgrade_sockets(
                current_spirit.upgrade_socket_path(),
                next_spirit.upgrade_socket_path(),
            ),
        ))
        .await
        .expect("owner attempt handover request succeeds against real spirit daemon");

    match reply {
        OwnerReply::HandoverSucceeded(success) => {
            assert_eq!(success.component.as_str(), "persona-spirit");
            assert_eq!(success.active_version.label.as_str(), "v0.1.1");
            assert_eq!(success.commit_sequence, 0);
        }
        other => panic!("expected handover success reply, got {other:?}"),
    }

    let served = current_thread
        .join()
        .expect("current spirit upgrade socket thread joins")
        .expect("current spirit daemon served three upgrade frames");
    assert_eq!(served.len(), 3);
    let next_served = next_thread
        .join()
        .expect("next spirit upgrade socket thread joins")
        .expect("next spirit daemon served one upgrade frame");
    assert_eq!(next_served.len(), 1);

    persona.stop_daemon();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_persona_daemon_hands_over_between_copied_spirit_databases() {
    let current_spirit = SpiritUpgradeSocketFixture::new("copied-current-database");
    let next_spirit = SpiritUpgradeSocketFixture::new("copied-next-database");
    let current_configuration = current_spirit.configuration();
    let current_ordinary_socket = current_spirit.ordinary_socket_path().to_path_buf();
    let current_thread = std::thread::spawn(move || -> persona_spirit::Result<(usize, bool)> {
        let mut daemon = SpiritDaemonRuntime::from_configuration(current_configuration).bind()?;
        daemon.serve_one()?;
        for _ in 0..3 {
            daemon.serve_upgrade_one()?;
        }
        let ordinary_socket_exists_after_handover = current_ordinary_socket.exists();
        daemon.shutdown()?;
        Ok((3, ordinary_socket_exists_after_handover))
    });
    wait_for_socket(current_spirit.ordinary_socket_path()).await;

    let record_reply = current_spirit
        .client()
        .submit(SpiritOperation::Record(spirit_entry(
            "copied before persona handover",
        )))
        .expect("current spirit accepts pre-handover record");
    assert_eq!(
        record_reply,
        SpiritReply::RecordAccepted(RecordAccepted::new(RecordIdentifier::new(1)))
    );

    next_spirit.copy_store_from(&current_spirit);
    let next_configuration = next_spirit.configuration();
    let next_thread = std::thread::spawn(move || -> persona_spirit::Result<(usize, usize)> {
        let mut daemon = SpiritDaemonRuntime::from_configuration(next_configuration).bind()?;
        daemon.serve_upgrade_one()?;
        daemon.serve_one()?;
        daemon.shutdown()?;
        Ok((1, 1))
    });
    wait_for_socket(current_spirit.upgrade_socket_path()).await;
    wait_for_socket(next_spirit.upgrade_socket_path()).await;

    let mut persona = DaemonFixture::start();
    let reply = persona
        .owner_client()
        .submit(OwnerOperation::AttemptHandover(
            owner_attempt_handover_order_with_upgrade_sockets(
                current_spirit.upgrade_socket_path(),
                next_spirit.upgrade_socket_path(),
            ),
        ))
        .await
        .expect("persona drives handover between copied spirit databases");

    match reply {
        OwnerReply::HandoverSucceeded(success) => {
            assert_eq!(success.component.as_str(), "persona-spirit");
            assert_eq!(success.active_version.label.as_str(), "v0.1.1");
            assert_eq!(success.commit_sequence, 1);
        }
        other => panic!("expected handover success reply, got {other:?}"),
    }

    let observed = next_spirit
        .client()
        .submit(observe_spirit_summaries())
        .expect("next spirit serves copied state after handover");
    assert_eq!(
        observed,
        SpiritReply::RecordsObserved(RecordsObserved {
            records: vec![RecordSummary {
                identifier: RecordIdentifier::new(1),
                topic: Topic::new("workspace"),
                kind: Kind::Decision,
                summary: Summary::new("copied before persona handover"),
                certainty: Magnitude::Maximum,
            }],
        })
    );

    let (current_upgrade_exchanges, current_public_socket_exists) = current_thread
        .join()
        .expect("current spirit thread joins")
        .expect("current spirit daemon served handover");
    assert_eq!(current_upgrade_exchanges, 3);
    assert!(
        !current_public_socket_exists,
        "current ordinary socket is removed after handover completion"
    );
    let (next_upgrade_exchanges, next_public_exchanges) = next_thread
        .join()
        .expect("next spirit thread joins")
        .expect("next spirit daemon served marker and copied-state query");
    assert_eq!(next_upgrade_exchanges, 1);
    assert_eq!(next_public_exchanges, 1);

    persona.stop_daemon();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_persona_daemon_handover_uses_real_spirit_daemon_binaries() {
    let Some(spirit_daemon_binary) = external_spirit_daemon_binary() else {
        eprintln!(
            "skipping real Spirit daemon binary witness; PERSONA_SPIRIT_DAEMON_BIN is not set"
        );
        return;
    };

    let current_spirit = SpiritUpgradeSocketFixture::new("external-current-database");
    let next_spirit = SpiritUpgradeSocketFixture::new("external-next-database");
    let current_configuration = current_spirit.configuration();
    let mut current_daemon =
        ExternalSpiritDaemon::spawn(&spirit_daemon_binary, &current_configuration);
    wait_for_external_spirit_socket(&mut current_daemon, current_spirit.ordinary_socket_path())
        .await;
    wait_for_external_spirit_socket(&mut current_daemon, current_spirit.upgrade_socket_path())
        .await;

    let record_reply = current_spirit
        .client()
        .submit(SpiritOperation::Record(spirit_entry(
            "real binary before persona handover",
        )))
        .expect("current external spirit daemon accepts pre-handover record");
    assert_eq!(
        record_reply,
        SpiritReply::RecordAccepted(RecordAccepted::new(RecordIdentifier::new(1)))
    );

    next_spirit.copy_store_from(&current_spirit);
    let next_configuration = next_spirit.configuration();
    let mut next_daemon = ExternalSpiritDaemon::spawn(&spirit_daemon_binary, &next_configuration);
    wait_for_external_spirit_socket(&mut next_daemon, next_spirit.ordinary_socket_path()).await;
    wait_for_external_spirit_socket(&mut next_daemon, next_spirit.upgrade_socket_path()).await;

    let mut persona = DaemonFixture::start();
    let reply = persona
        .owner_client()
        .submit(OwnerOperation::AttemptHandover(
            owner_attempt_handover_order_with_upgrade_sockets(
                current_spirit.upgrade_socket_path(),
                next_spirit.upgrade_socket_path(),
            ),
        ))
        .await
        .expect("persona drives handover between external spirit daemon binaries");

    match reply {
        OwnerReply::HandoverSucceeded(success) => {
            assert_eq!(success.component.as_str(), "persona-spirit");
            assert_eq!(success.active_version.label.as_str(), "v0.1.1");
            assert_eq!(success.commit_sequence, 1);
        }
        other => panic!("expected handover success reply, got {other:?}"),
    }

    let observed = next_spirit
        .client()
        .submit(observe_spirit_summaries())
        .expect("next external spirit daemon serves copied state after handover");
    assert_eq!(
        observed,
        SpiritReply::RecordsObserved(RecordsObserved {
            records: vec![RecordSummary {
                identifier: RecordIdentifier::new(1),
                topic: Topic::new("workspace"),
                kind: Kind::Decision,
                summary: Summary::new("real binary before persona handover"),
                certainty: Magnitude::Maximum,
            }],
        })
    );

    persona.stop_daemon();
    let store = persona::manager_store::ManagerStore::start(
        persona::manager_store::ManagerStoreLocation::new(&persona.manager_store),
    )
    .expect("manager store starts for inspection");
    let active = store
        .ask(persona::manager_store::ReadActiveVersion::new(
            signal_persona_auth::EngineId::new("default"),
            signal_persona::ComponentName::new("persona-spirit"),
        ))
        .await
        .expect("active version read")
        .expect("active version persisted");
    assert_eq!(active.active_version().as_str(), "v0.1.1");
    assert_eq!(active.commit_sequence(), Some(1));

    let events = store
        .ask(persona::manager_store::ReadEngineEvents::new(
            signal_persona_auth::EngineId::new("default"),
        ))
        .await
        .expect("engine events read");
    assert!(
        events.iter().any(|event| {
            matches!(
                event.body(),
                EngineEventBody::ActiveVersionChanged(change)
                    if change.active_version().as_str() == "v0.1.1"
                        && change.commit_sequence() == Some(1)
            )
        }),
        "ActiveVersionChanged event not recorded in manager store"
    );

    store.stop_gracefully().await.expect("manager store stops");
    let _shutdown_completion = store.wait_for_shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_persona_daemon_recovers_real_spirit_after_completion_failure() {
    let Some(spirit_daemon_binary) = external_spirit_daemon_binary() else {
        eprintln!(
            "skipping real Spirit daemon recovery witness; PERSONA_SPIRIT_DAEMON_BIN is not set"
        );
        return;
    };

    let current_spirit = SpiritUpgradeSocketFixture::new("external-current-recovery");
    let next_spirit = SpiritUpgradeSocketFixture::new("external-next-recovery");
    let current_configuration = current_spirit.configuration();
    let mut current_daemon =
        ExternalSpiritDaemon::spawn(&spirit_daemon_binary, &current_configuration);
    wait_for_external_spirit_socket(&mut current_daemon, current_spirit.ordinary_socket_path())
        .await;
    wait_for_external_spirit_socket(&mut current_daemon, current_spirit.upgrade_socket_path())
        .await;

    let record_reply = current_spirit
        .client()
        .submit(SpiritOperation::Record(spirit_entry(
            "real binary before failed persona handover",
        )))
        .expect("current external spirit daemon accepts pre-handover record");
    assert_eq!(
        record_reply,
        SpiritReply::RecordAccepted(RecordAccepted::new(RecordIdentifier::new(1)))
    );

    next_spirit.copy_store_from(&current_spirit);
    let next_configuration = next_spirit.configuration();
    let mut next_daemon = ExternalSpiritDaemon::spawn(&spirit_daemon_binary, &next_configuration);
    wait_for_external_spirit_socket(&mut next_daemon, next_spirit.ordinary_socket_path()).await;
    wait_for_external_spirit_socket(&mut next_daemon, next_spirit.upgrade_socket_path()).await;

    let mut persona = DaemonFixture::start();
    let forced = persona
        .owner_client()
        .submit(OwnerOperation::ForceFlip(owner_force_current_order()))
        .await
        .expect("owner force flip pins the current active version before failed handover");
    match forced {
        OwnerReply::FlipForced(reply) => {
            assert_eq!(reply.component.as_str(), "persona-spirit");
            assert_eq!(reply.active_version.label.as_str(), "v0.1.0");
        }
        other => panic!("expected force flip reply, got {other:?}"),
    }

    let current_proxy_socket = persona.root.join("spirit-current-recovery-proxy.sock");
    let current_proxy = tokio::spawn(serve_recovering_current_proxy(
        current_proxy_socket.clone(),
        current_spirit.upgrade_socket_path().to_path_buf(),
    ));
    wait_for_socket(&current_proxy_socket).await;

    let reply = persona
        .owner_client()
        .submit(OwnerOperation::AttemptHandover(
            owner_attempt_handover_order_with_upgrade_sockets(
                &current_proxy_socket,
                next_spirit.upgrade_socket_path(),
            ),
        ))
        .await
        .expect("persona owner handover returns typed failure after real Spirit recovery");
    match reply {
        OwnerReply::Rejected(rejected) => {
            assert_eq!(rejected.component.as_str(), "persona-spirit");
            assert_eq!(rejected.reason, RejectionReason::HandoverRejected);
        }
        other => panic!("expected handover rejection reply, got {other:?}"),
    }

    let operations = current_proxy
        .await
        .expect("current Spirit recovery proxy joins")
        .expect("current Spirit recovery proxy succeeds");
    assert!(matches!(
        operations.as_slice(),
        [
            HandoverOperation::AskHandoverMarker(_),
            HandoverOperation::ReadyToHandover(_),
            HandoverOperation::HandoverCompleted(_),
            HandoverOperation::RecoverFromFailure(_),
        ]
    ));

    let recovered_record = current_spirit
        .client()
        .submit(SpiritOperation::Record(spirit_entry(
            "write after failed persona handover recovery",
        )))
        .expect("current external spirit daemon resumes ordinary writes after recovery");
    assert_eq!(
        recovered_record,
        SpiritReply::RecordAccepted(RecordAccepted::new(RecordIdentifier::new(2)))
    );

    let observed = current_spirit
        .client()
        .submit(observe_spirit_summaries())
        .expect("current external spirit daemon serves recovered copied state");
    assert_eq!(
        observed,
        SpiritReply::RecordsObserved(RecordsObserved {
            records: vec![
                RecordSummary {
                    identifier: RecordIdentifier::new(1),
                    topic: Topic::new("workspace"),
                    kind: Kind::Decision,
                    summary: Summary::new("real binary before failed persona handover"),
                    certainty: Magnitude::Maximum,
                },
                RecordSummary {
                    identifier: RecordIdentifier::new(2),
                    topic: Topic::new("workspace"),
                    kind: Kind::Decision,
                    summary: Summary::new("write after failed persona handover recovery"),
                    certainty: Magnitude::Maximum,
                },
            ],
        })
    );

    persona.stop_daemon();
    let store = persona::manager_store::ManagerStore::start(
        persona::manager_store::ManagerStoreLocation::new(&persona.manager_store),
    )
    .expect("manager store starts for inspection");
    let active = store
        .ask(persona::manager_store::ReadActiveVersion::new(
            signal_persona_auth::EngineId::new("default"),
            signal_persona::ComponentName::new("persona-spirit"),
        ))
        .await
        .expect("active version read")
        .expect("force-flipped active version persisted");
    assert_eq!(active.active_version().as_str(), "v0.1.0");
    assert_eq!(active.commit_sequence(), None);

    let events = store
        .ask(persona::manager_store::ReadEngineEvents::new(
            signal_persona_auth::EngineId::new("default"),
        ))
        .await
        .expect("engine events read");
    assert!(
        !events.iter().any(|event| {
            matches!(
                event.body(),
                EngineEventBody::ActiveVersionChanged(change)
                    if change.active_version().as_str() == "v0.1.1"
            )
        }),
        "failed handover must not persist an ActiveVersionChanged event for v0.1.1"
    );

    store.stop_gracefully().await.expect("manager store stops");
    let _shutdown_completion = store.wait_for_shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_persona_daemon_launches_message_router_topology_through_engine_supervisor() {
    let mut fixture = DaemonFixture::start_with_message_router_components();
    let topology = EngineTopology::MessageRouter;

    for component in topology.components().iter().copied() {
        let capture = fixture.wait_for_component_capture(component);
        assert!(
            capture.contains("engine=default"),
            "capture for {component:?}: {capture}"
        );
        assert!(
            capture.contains(&format!("component={}", component.as_str())),
            "capture for {component:?}: {capture}"
        );
        assert!(
            capture.contains("peer_count=1"),
            "capture for {component:?}: {capture}"
        );
    }
    assert!(
        !fixture.component_capture(EngineComponent::Mind).exists(),
        "message-router topology must not launch persona-mind"
    );

    fixture.stop_daemon();

    let store = persona::manager_store::ManagerStore::start(
        persona::manager_store::ManagerStoreLocation::new(&fixture.manager_store),
    )
    .expect("manager store starts for inspection");
    let events = store
        .ask(persona::manager_store::ReadEngineEvents::new(
            signal_persona_auth::EngineId::new("default"),
        ))
        .await
        .expect("default engine events read through manager store actor");
    assert_eq!(events.len(), topology.components().len() * 2);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.body(), EngineEventBody::ComponentSpawned(_)))
            .count(),
        topology.components().len()
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.body(), EngineEventBody::ComponentReady(_)))
            .count(),
        topology.components().len()
    );

    store.stop_gracefully().await.expect("manager store stops");
    let _shutdown_completion = store.wait_for_shutdown().await;
}

#[test]
fn constraint_persona_cli_talks_to_persona_daemon_over_socket() {
    let fixture = DaemonFixture::start();

    let shutdown = fixture.persona("(ComponentShutdown persona-terminal)");
    assert!(shutdown.contains("(ActionAcceptedReport persona-terminal Stopped)"));

    let status = fixture.persona("(ComponentStatusQuery persona-terminal)");
    assert!(status.contains("(ComponentStatusReport "));
    assert!(status.contains("(persona-terminal Terminal Stopped Stopped)"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_persona_daemon_persists_cli_mutation_to_manager_store() {
    let mut fixture = DaemonFixture::start();

    let shutdown = fixture.persona("(ComponentShutdown persona-terminal)");
    assert!(shutdown.contains("(ActionAcceptedReport persona-terminal Stopped)"));

    fixture.stop_daemon();

    let store = persona::manager_store::ManagerStore::start(
        persona::manager_store::ManagerStoreLocation::new(&fixture.manager_store),
    )
    .expect("manager store starts for inspection");
    let record = store
        .ask(persona::manager_store::ReadEngineRecord::new(
            signal_persona_auth::EngineId::new("default"),
        ))
        .await
        .expect("stored record read through manager store actor")
        .expect("default engine record exists");
    let terminal = record
        .status()
        .components
        .iter()
        .find(|component| component.name.as_str() == "persona-terminal")
        .expect("terminal component stored");
    assert_eq!(
        terminal.desired_state,
        signal_persona::ComponentDesiredState::Stopped
    );

    store.stop_gracefully().await.expect("manager store stops");
    let _shutdown_completion = store.wait_for_shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn constraint_persona_daemon_launches_prototype_supervised_components_through_engine_supervisor()
 {
    let mut fixture = DaemonFixture::start_with_prototype_supervised_components();

    for component in EngineComponent::prototype_supervised_components() {
        let capture = fixture.wait_for_component_capture(component);
        assert!(
            capture.contains("engine=default"),
            "capture for {component:?}: {capture}"
        );
        assert!(
            capture.contains(&format!("component={}", component.as_str())),
            "capture for {component:?}: {capture}"
        );
        assert!(
            capture.contains("spawn_envelope="),
            "capture for {component:?}: {capture}"
        );
        assert!(
            capture.contains(component.envelope_file()),
            "capture for {component:?}: {capture}"
        );
        assert!(
            capture.contains("manager_socket="),
            "capture for {component:?}: {capture}"
        );
        assert!(
            capture.contains("domain_socket="),
            "capture for {component:?}: {capture}"
        );
        assert!(
            capture.contains("supervision_socket="),
            "capture for {component:?}: {capture}"
        );
        assert!(
            capture.contains(&format!(
                "domain_mode={:o}",
                component.socket_mode().as_octal()
            )),
            "capture for {component:?}: {capture}"
        );
        assert!(
            capture.contains(&format!(
                "supervision_mode={:o}",
                component.supervision_socket_mode().as_octal()
            )),
            "capture for {component:?}: {capture}"
        );
        assert!(
            capture.contains("peer_count=6"),
            "capture for {component:?}: {capture}"
        );
    }

    fixture.stop_daemon();

    let store = persona::manager_store::ManagerStore::start(
        persona::manager_store::ManagerStoreLocation::new(&fixture.manager_store),
    )
    .expect("manager store starts for inspection");
    let events = store
        .ask(persona::manager_store::ReadEngineEvents::new(
            signal_persona_auth::EngineId::new("default"),
        ))
        .await
        .expect("default engine events read through manager store actor");
    assert_eq!(
        events.len(),
        EngineComponent::prototype_supervised_components().len() * 2
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.body(), EngineEventBody::ComponentSpawned(_)))
            .count(),
        EngineComponent::prototype_supervised_components().len()
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.body(), EngineEventBody::ComponentReady(_)))
            .count(),
        EngineComponent::prototype_supervised_components().len()
    );

    store.stop_gracefully().await.expect("manager store stops");
    let _shutdown_completion = store.wait_for_shutdown().await;
}

#[test]
fn constraint_persona_daemon_does_not_delete_non_socket_endpoint_path() {
    let root = std::env::temp_dir().join(format!(
        "persona-daemon-occupied-path-test-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("test root created");
    let endpoint = root.join("persona.sock");
    std::fs::write(&endpoint, "not a socket").expect("regular file created");

    let output = Command::new(env!("CARGO_BIN_EXE_persona-daemon"))
        .arg(&endpoint)
        .output()
        .expect("persona-daemon runs");

    assert!(
        !output.status.success(),
        "persona-daemon should reject occupied path"
    );
    assert_eq!(
        std::fs::read_to_string(&endpoint).expect("regular file preserved"),
        "not a socket"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("non-socket file"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let _ = std::fs::remove_dir_all(&root);
}
