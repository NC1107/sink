//! The PipeWire main-loop thread. All PipeWire objects live here (they are
//! not Send); the `PipeWireBackend` facade talks to this thread through a
//! pipewire channel, and each command carries an mpsc reply sender.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::Arc;

use pipewire as pw;
use pw::core::CoreRc;
use pw::metadata::{Metadata, MetadataListener};
use pw::node::{Node, NodeListener};
use pw::registry::{GlobalObject, RegistryRc};
use pw::spa::utils::dict::DictRef;
use pw::types::ObjectType;

use crate::audio::pw_native::eq_chain::EqChainHandle;
use crate::audio::pw_native::levels::LevelStore;
use crate::audio::pw_native::meter::MeterHandle;
use crate::audio::pw_native::mic::{MicStreams, MIC_NODE};
use crate::audio::pw_native::pods;
use crate::audio::types::{is_virtual_sink, AppStream, EqConfig, MicConfig, OutputDevice};
use crate::error::SinkError;
use crate::persistence::buses::is_bus_name;

const STREAM_CLASS: &str = "Stream/Output/Audio";
const SINK_CLASS: &str = "Audio/Sink";
const SOURCE_CLASS: &str = "Audio/Source";
const VIRTUAL_SOURCE_CLASS: &str = "Audio/Source/Virtual";
/// node.name prefix of all our internal helper streams (meters, mic chain) —
/// excluded from stream listings and node tracking.
pub const INTERNAL_PREFIX: &str = "sink-internal-";
/// node.name prefix of our meter capture streams.
pub const METER_PREFIX: &str = "sink-internal-meter-";

type Reply<T> = mpsc::Sender<Result<T, SinkError>>;
/// A set of live links: (output port, input port, proxy).
type LinkSet = Vec<(u32, u32, pw::link::Link)>;

pub enum Cmd {
    CreateSink { name: String, label: String, reply: Reply<()> },
    DestroySink { name: String, reply: Reply<()> },
    ListStreams { reply: Reply<Vec<AppStream>> },
    ListOutputs { reply: Reply<Vec<OutputDevice>> },
    ResolvedOutputs { reply: Reply<HashMap<String, Option<String>>> },
    SetNodeVolumeByName { name: String, percent: u8, reply: Reply<()> },
    SetNodeMuteByName { name: String, muted: bool, reply: Reply<()> },
    SetNodeVolumeById { id: u32, percent: u8, reply: Reply<()> },
    MoveStream { id: u32, sink_name: String, reply: Reply<()> },
    /// Route a channel's monitor to an output device (None = follow default).
    SetChannelOutput { sink_name: String, output_name: Option<String>, reply: Reply<()> },
    SetChannelFailover { sink_name: String, enabled: bool, reply: Reply<()> },
    /// Create a mix bus (capturable virtual source).
    CreateBus { name: String, label: String, reply: Reply<()> },
    /// Destroy a mix bus and its links.
    DestroyBus { name: String, reply: Reply<()> },
    /// Replace the channel set feeding a bus.
    SetBusMembers { name: String, channels: Vec<String>, reply: Reply<()> },
    /// Listen to a channel/mix/mic on the default output (session scoped).
    SetMonitor { name: String, enabled: bool, reply: Reply<()> },
    /// Apply mic chain configuration (create/destroy/re-tune as needed).
    SetMicConfig { config: MicConfig, reply: Reply<()> },
    /// Apply a channel's parametric EQ (create/destroy/re-tune the insert).
    SetChannelEq { sink_name: String, config: EqConfig, reply: Reply<()> },
    /// Hardware capture devices (microphones).
    ListInputs { reply: Reply<Vec<OutputDevice>> },
    /// Current system defaults: (output sink name, input source name).
    GetDefaults { reply: Reply<(Option<String>, Option<String>)> },
    /// Set the configured system default sink (input=false) or source.
    SetDefault { input: bool, name: String, reply: Reply<()> },
}

struct PortEntry {
    id: u32,
    node_id: u32,
    /// "in" (playback/sink input port) or "out" (source/monitor port).
    direction: String,
    /// e.g. "FL", "FR", "MONO".
    channel: Option<String>,
}

struct NodeEntry {
    id: u32,
    serial: Option<u64>,
    media_class: String,
    props: HashMap<String, String>,
    proxy: Node,
    _listener: NodeListener,
    volume_percent: u8,
    channels: usize,
    muted: bool,
    /// True while the node is in the Running state (actively streaming).
    active: bool,
}

#[derive(Default)]
struct State {
    nodes: HashMap<u32, NodeEntry>,
    /// link global id -> (output node id, input node id)
    links: HashMap<u32, (u32, u32)>,
    metadata: Option<Metadata>,
    _metadata_listener: Option<MetadataListener>,
    default_sink_name: Option<String>,
    default_source_name: Option<String>,
    /// Virtual sinks we created: name -> created-object proxy (kept alive;
    /// destroyed explicitly on teardown).
    owned_sinks: HashMap<String, Node>,
    /// Sinks that existed before us (e.g. leftover pactl modules): name -> global id.
    adopted_sinks: HashMap<String, u32>,
    /// Nodes that must stay alive: name -> (label, kind 0=sink 1=bus 2=mic).
    /// If one vanishes without us destroying it (another instance dying,
    /// a PipeWire restart, wpctl) it gets recreated on the spot.
    desired: HashMap<String, (String, u8)>,
    /// Create requests waiting for the sink's global to appear.
    pending_creates: HashMap<String, Vec<Reply<()>>>,
    /// Live meter capture streams per virtual sink name.
    meters: HashMap<String, MeterHandle>,
    /// All known ports, for monitor→output linking.
    ports: HashMap<u32, PortEntry>,
    /// Channel sink name -> chosen output node.name (None = follow default).
    channel_outputs: HashMap<String, Option<String>>,

    /// Channel sink name -> live loopback links.
    channel_links: HashMap<String, LinkSet>,
    /// Channel sink name -> the device node id it currently routes to (after
    /// explicit/default/fallback resolution). Lets the UI show what "System
    /// default" actually resolves to, and makes failover visible.
    channel_targets: HashMap<String, u32>,
    /// Channels with auto-failover turned off: they route only to their chosen
    /// device (or the exact default) and stay silent when it's gone. Absence
    /// (the default) means failover is on.
    channel_strict: std::collections::HashSet<String>,
    /// Phase 3 mic chain.
    mic_config: MicConfig,
    /// Proxy for the sink_mic virtual source (kept alive while enabled).
    mic_source: Option<Node>,
    /// Mic-node removals we caused ourselves (a rename recreates the node), so
    /// the heal path can tell them from an external destroy and only recreate
    /// for the latter.
    mic_expected_removals: u32,
    mic_streams: Option<MicStreams>,
    levels: Option<Arc<LevelStore>>,
    /// Mix buses we own: node name -> proxy.
    bus_sources: HashMap<String, Node>,
    /// Bus node name -> member channel sink names.
    bus_members: HashMap<String, std::collections::HashSet<String>>,
    /// (bus, channel) -> live links feeding the bus.
    bus_links: HashMap<(String, String), LinkSet>,
    /// Nodes monitored on the default output, and their live links.
    monitored: std::collections::HashSet<String>,
    monitor_links: HashMap<String, LinkSet>,
    /// Links from the mic playback stream into the virtual mic.
    mic_links: LinkSet,
    /// Per-channel EQ configs (source of truth for chain (re)creation —
    /// kept even while disabled so re-enabling restores the bands).
    eq_configs: HashMap<String, EqConfig>,
    /// Live EQ inserts by channel sink name. Presence here *is* "EQ is
    /// enabled and live"; the channel's outgoing links re-source from the
    /// insert's playback node.
    eq_streams: HashMap<String, EqChainHandle>,
    /// EQ playback node id -> node ids it is allowed to feed. Rebuilt
    /// wholesale by every `ensure_all_links` pass; the link police destroys
    /// anything else (WirePlumber routes playback streams to the default
    /// sink — same leak the mic police exists for).
    eq_desired_targets: HashMap<u32, std::collections::HashSet<u32>>,
}

impl State {
    /// Live node id of the mic playback stream. Resolved lazily — the id
    /// is only valid once the server has created the stream's node.
    fn mic_playback_node(&self) -> Option<u32> {
        self.mic_streams
            .as_ref()
            .map(|m| m.playback_node_id())
            .filter(|id| *id != u32::MAX)
    }

    /// Live node id of a channel's EQ playback stream, if the insert is up.
    fn eq_playback_node(&self, sink_name: &str) -> Option<u32> {
        self.eq_streams
            .get(sink_name)
            .map(|h| h.playback_node_id())
            .filter(|id| *id != u32::MAX)
    }
}

/// The node whose ports feed a channel's downstream links: the EQ insert's
/// playback stream when one is live, otherwise the channel sink itself.
/// Pure so the routing decision is unit-testable (like `resolve_target`).
fn resolve_source(eq_playback: Option<u32>, channel_id: u32) -> u32 {
    eq_playback.unwrap_or(channel_id)
}

impl State {
    fn node_by_name(&self, name: &str) -> Option<&NodeEntry> {
        self.nodes
            .values()
            .find(|n| n.props.get("node.name").map(String::as_str) == Some(name))
    }

    /// The sink a stream is currently connected to, resolved through links.
    fn sink_of_stream(&self, stream_id: u32) -> Option<&NodeEntry> {
        self.links
            .values()
            .find(|(out, _)| *out == stream_id)
            .and_then(|(_, input)| self.nodes.get(input))
    }
}

// The CoreRc is needed by the command handler (object creation/destruction);
// this thread owns all PipeWire objects, so a thread-local is the simplest
// way to share it across the listener closures.
thread_local! {
    static CORE: RefCell<Option<CoreRc>> = const { RefCell::new(None) };
}

/// Entry point: runs the PipeWire loop until the channel closes.
/// `init_tx` reports startup success/failure exactly once.
pub fn run(
    receiver: pw::channel::Receiver<Cmd>,
    init_tx: mpsc::Sender<Result<(), SinkError>>,
    levels: Arc<LevelStore>,
) {
    if let Err(e) = setup_and_run(receiver, &init_tx, levels) {
        let _ = init_tx.send(Err(e));
    }
}

fn setup_and_run(
    receiver: pw::channel::Receiver<Cmd>,
    init_tx: &mpsc::Sender<Result<(), SinkError>>,
    levels: Arc<LevelStore>,
) -> Result<(), SinkError> {
    pw::init();
    let err = |stage: &str, e: pw::Error| SinkError::Config(format!("pipewire {stage}: {e}"));

    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(|e| err("mainloop", e))?;
    let context = pw::context::ContextRc::new(&mainloop, None).map_err(|e| err("context", e))?;
    let core = context.connect_rc(None).map_err(|e| err("connect", e))?;
    let registry = core.get_registry_rc().map_err(|e| err("registry", e))?;

    CORE.with(|c| *c.borrow_mut() = Some(core.clone()));

    let state = Rc::new(RefCell::new(State {
        levels: Some(levels.clone()),
        ..State::default()
    }));

    // ---- registry listeners ----
    let state_g = state.clone();
    let registry_g = registry.clone();
    let core_g = core.clone();
    let levels_g = levels.clone();
    let _reg_listener = registry
        .add_listener_local()
        .global(move |global| {
            on_global(&state_g, &registry_g, &core_g, &levels_g, global);
        })
        .global_remove({
            let state = state.clone();
            move |id| {
                enum Heal {
                    Nothing,
                    Relink,
                    Recreate(String, String, u8),
                }
                let heal = {
                    let mut s = state.borrow_mut();
                    s.links.remove(&id);
                    s.ports.remove(&id);
                    let Some(node) = s.nodes.remove(&id) else {
                        return;
                    };
                    let name = node.props.get("node.name").cloned().unwrap_or_default();
                    if node.media_class == SINK_CLASS {
                        s.meters.remove(&name);
                        s.adopted_sinks.remove(&name);
                    }
                    match s.desired.get(&name).cloned() {
                        Some((label, kind)) => {
                            // Drop any dangling proxy so the heal isn't
                            // blocked by a corpse.
                            match kind {
                                0 => {
                                    s.owned_sinks.remove(&name);
                                }
                                1 => {
                                    s.bus_sources.remove(&name);
                                }
                                _ => {}
                            }
                            // A deliberate recreate (mic rename) already has
                            // a fresh proxy/node — don't double up. For the
                            // mic the new proxy is set synchronously before
                            // this removal event, so `mic_source.is_some()`
                            // can't tell our own destroy from an external one;
                            // the expected-removals counter can.
                            let already_back = match kind {
                                2 => {
                                    if s.mic_expected_removals > 0 {
                                        s.mic_expected_removals -= 1;
                                        true
                                    } else {
                                        // External destroy (wpctl, a session
                                        // hiccup): drop the dead proxy so the
                                        // recreate below isn't blocked by it.
                                        s.mic_source = None;
                                        false
                                    }
                                }
                                _ => s.node_by_name(&name).is_some(),
                            };
                            if already_back {
                                Heal::Relink
                            } else {
                                s.meters.remove(&name);
                                Heal::Recreate(name, label, kind)
                            }
                        }
                        // An output device vanished: relink so affected
                        // channels fail over to the default.
                        None if node.media_class == SINK_CLASS => Heal::Relink,
                        None => Heal::Nothing,
                    }
                };
                match heal {
                    Heal::Recreate(name, label, kind) => {
                        eprintln!("sink: {name} vanished externally — recreating");
                        if let Some(core) = CORE.with(|c| c.borrow().clone()) {
                            match create_node_object(&core, &name, &label, kind) {
                                Ok(proxy) => {
                                    let mut s = state.borrow_mut();
                                    match kind {
                                        0 => {
                                            s.owned_sinks.insert(name, proxy);
                                        }
                                        1 => {
                                            s.bus_sources.insert(name, proxy);
                                        }
                                        _ => s.mic_source = Some(proxy),
                                    }
                                }
                                Err(e) => eprintln!("sink: recreate {name} failed: {e}"),
                            }
                        }
                        ensure_all_links(&state);
                        ensure_mic_links(&state);
                    }
                    Heal::Relink => ensure_all_links(&state),
                    Heal::Nothing => {}
                }
            }
        })
        .register();

    // ---- command channel ----
    let state_c = state.clone();
    let registry_c = registry.clone();
    let _recv = receiver.attach(mainloop.loop_(), move |cmd| {
        handle_cmd(&state_c, &registry_c, cmd);
    });

    init_tx
        .send(Ok(()))
        .map_err(|_| SinkError::Config("backend owner vanished during init".into()))?;

    mainloop.run();
    Ok(())
}

fn on_global(
    state: &Rc<RefCell<State>>,
    registry: &RegistryRc,
    core: &CoreRc,
    levels: &Arc<LevelStore>,
    global: &GlobalObject<&DictRef>,
) {
    match global.type_ {
        ObjectType::Node => on_node(state, registry, core, levels, global),
        ObjectType::Port => {
            let Some(props) = global.props else { return };
            let Some(node_id) = props.get("node.id").and_then(|v| v.parse().ok()) else {
                return;
            };
            let entry = PortEntry {
                id: global.id,
                node_id,
                direction: props.get("port.direction").unwrap_or_default().to_string(),
                channel: props.get("audio.channel").map(str::to_string),
            };
            state.borrow_mut().ports.insert(global.id, entry);
            // Channel and mic wiring both depend on ports of untracked
            // stream nodes (EQ/mic playback streams), so reconcile on every
            // port event — both are idempotent no-ops until both ends exist.
            ensure_all_links(state);
            ensure_mic_links(state);
        }
        ObjectType::Link => {
            let Some(props) = global.props else { return };
            let out = props.get("link.output.node").and_then(|v| v.parse().ok());
            let inp = props.get("link.input.node").and_then(|v| v.parse().ok());
            if let (Some(out), Some(inp)) = (out, inp) {
                let police = {
                    let mut s = state.borrow_mut();
                    s.links.insert(global.id, (out, inp));
                    // Police the mic playback stream: if anything (e.g. a
                    // session-manager fallback) links it somewhere other
                    // than the virtual mic, destroy that link — mic audio
                    // must never leak into the speakers.
                    let mic_stray = match (s.mic_playback_node(), s.node_by_name(MIC_NODE)) {
                        (Some(playback), mic) if out == playback => {
                            mic.map(|n| n.id) != Some(inp)
                        }
                        _ => false,
                    };
                    // Same policing for EQ playback streams: only the links
                    // the loop planned (device/buses/monitor) may exist. An
                    // EQ node with no plan yet (chain just built, first
                    // reconcile pending) allows nothing — our own links are
                    // always created after the plan is recorded.
                    let eq_stray = s
                        .eq_streams
                        .values()
                        .any(|h| h.playback_node_id() == out)
                        && !s
                            .eq_desired_targets
                            .get(&out)
                            .is_some_and(|allowed| allowed.contains(&inp));
                    mic_stray || eq_stray
                };
                if police {
                    let _ = registry.destroy_global(global.id);
                }
            }
        }
        ObjectType::Metadata => {
            let Some(props) = global.props else { return };
            if props.get("metadata.name") != Some("default") {
                return;
            }
            let Ok(metadata) = registry.bind::<Metadata, _>(global) else {
                return;
            };
            let state_m = state.clone();
            let listener = metadata
                .add_listener_local()
                .property(move |_subject, key, _type, value| {
                    // values are JSON like {"name":"alsa_output...."}
                    let parse_name = |v: Option<&str>| {
                        v.and_then(|v| {
                            serde_json::from_str::<serde_json::Value>(v)
                                .ok()?
                                .get("name")?
                                .as_str()
                                .map(str::to_string)
                        })
                    };
                    if key == Some("default.audio.sink") {
                        let name = parse_name(value);
                        let changed = {
                            let mut s = state_m.borrow_mut();
                            let changed = s.default_sink_name != name;
                            s.default_sink_name = name;
                            changed
                        };
                        // Channels following the default must relink
                        // (Sonar-style automatic device failover).
                        if changed {
                            ensure_all_links(&state_m);
                        }
                    } else if key == Some("default.audio.source") {
                        let name = parse_name(value);
                        let rebuild = {
                            let mut s = state_m.borrow_mut();
                            let changed = s.default_source_name != name;
                            s.default_source_name = name;
                            // A follow-default mic chain is pinned to the
                            // resolved device (dont-reconnect), so it
                            // tracks default changes by rebuilding.
                            changed
                                && s.mic_config.enabled
                                && s.mic_config.input_device.is_none()
                                && s.mic_streams.is_some()
                        };
                        if rebuild {
                            state_m.borrow_mut().mic_streams = None;
                            build_mic_streams(&state_m);
                        }
                    }
                    0
                })
                .register();
            let mut s = state.borrow_mut();
            s.metadata = Some(metadata);
            s._metadata_listener = Some(listener);
        }
        _ => {}
    }
}

fn on_node(
    state: &Rc<RefCell<State>>,
    registry: &RegistryRc,
    core: &CoreRc,
    levels: &Arc<LevelStore>,
    global: &GlobalObject<&DictRef>,
) {
    let Some(dict) = global.props else { return };
    let media_class = dict.get("media.class").unwrap_or_default().to_string();
    if media_class != STREAM_CLASS
        && media_class != SINK_CLASS
        && media_class != SOURCE_CLASS
        && media_class != VIRTUAL_SOURCE_CLASS
    {
        return;
    }
    let props: HashMap<String, String> = dict
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let node_name = props.get("node.name").cloned().unwrap_or_default();
    // Never track our own internal helper streams (meters, mic chain).
    if node_name.starts_with(INTERNAL_PREFIX) {
        return;
    }

    let Ok(proxy) = registry.bind::<Node, _>(global) else {
        return;
    };

    // Track volume/mute through Props param events, and the running state
    // through info events (drives the app-list activity indicator).
    let state_p = state.clone();
    let state_i = state.clone();
    let node_id = global.id;
    let listener = proxy
        .add_listener_local()
        .info(move |info| {
            let running = matches!(info.state(), pw::node::NodeState::Running);
            let mut s = state_i.borrow_mut();
            if let Some(entry) = s.nodes.get_mut(&node_id) {
                entry.active = running;
                // Registry globals only carry an abbreviated prop set; the
                // info event has the full dict (e.g. application.process.
                // binary, needed to name Discord's "WEBRTC VoiceEngine").
                if let Some(props) = info.props() {
                    for (k, v) in props.iter() {
                        entry.props.insert(k.to_string(), v.to_string());
                    }
                }
            }
        })
        .param(move |_seq, id, _index, _next, param| {
            if id != pw::spa::param::ParamType::Props {
                return;
            }
            let Some(pod) = param else { return };
            let parsed = pods::parse_props(pod);
            let mut s = state_p.borrow_mut();
            if let Some(entry) = s.nodes.get_mut(&node_id) {
                if let Some(linear) = parsed.volume_linear {
                    entry.volume_percent = pods::linear_to_percent(linear);
                }
                if let Some(channels) = parsed.channels {
                    entry.channels = channels;
                }
                if let Some(muted) = parsed.muted {
                    entry.muted = muted;
                }
            }
        })
        .register();
    proxy.subscribe_params(&[pw::spa::param::ParamType::Props]);

    let entry = NodeEntry {
        id: global.id,
        serial: props.get("object.serial").and_then(|v| v.parse().ok()),
        media_class: media_class.clone(),
        props,
        proxy,
        _listener: listener,
        volume_percent: 100,
        channels: 2,
        muted: false,
        active: false,
    };

    let mut s = state.borrow_mut();
    s.nodes.insert(global.id, entry);

    if media_class == SINK_CLASS && is_virtual_sink(&node_name) {
        // A virtual sink came up: resolve pending create requests, remember
        // it for teardown if we didn't create it, and attach a level meter.
        if let Some(waiters) = s.pending_creates.remove(&node_name) {
            for reply in waiters {
                let _ = reply.send(Ok(()));
            }
        }
        if !s.owned_sinks.contains_key(&node_name) {
            s.adopted_sinks.insert(node_name.clone(), global.id);
        }
        if !s.meters.contains_key(&node_name) {
            match MeterHandle::new(core, &node_name, global.id, levels.clone(), true) {
                Ok(meter) => {
                    s.meters.insert(node_name.clone(), meter);
                }
                Err(e) => eprintln!("sink: meter for {node_name} failed: {e}"),
            }
        }
        // An enabled EQ config with no live insert: build it against the
        // fresh sink id. Covers both startup (config loaded before the sink
        // exists) and the heal path (sink recreated after an external
        // destroy) with the same hook — like the meter above.
        if !s.eq_streams.contains_key(&node_name) {
            if let Some(config) = s.eq_configs.get(&node_name).filter(|c| c.enabled).cloned() {
                match EqChainHandle::new(core, &node_name, global.id, &config) {
                    Ok(handle) => {
                        s.eq_streams.insert(node_name.clone(), handle);
                    }
                    Err(e) => eprintln!("sink: eq chain for {node_name} failed: {e}"),
                }
            }
        }
        drop(s);
        ensure_all_links(state);
        return;
    }

    // The virtual mic source came up: attach the DSP streams.
    if media_class == VIRTUAL_SOURCE_CLASS && node_name == MIC_NODE {
        drop(s);
        build_mic_streams(state);
        return;
    }

    // A mix bus came up: meter it (direct source capture) and link members.
    if media_class == VIRTUAL_SOURCE_CLASS && is_bus_name(&node_name) {
        if !s.meters.contains_key(&node_name) {
            match MeterHandle::new(core, &node_name, global.id, levels.clone(), false) {
                Ok(meter) => {
                    s.meters.insert(node_name.clone(), meter);
                }
                Err(e) => eprintln!("sink: bus meter for {node_name} failed: {e}"),
            }
        }
        drop(s);
        ensure_all_links(state);
        return;
    }

    drop(s);
    // A new hardware sink may be the (returning) target of a channel.
    if media_class == SINK_CLASS {
        ensure_all_links(state);
    }
}

/// (Re)build the mic capture/DSP/playback streams. The loop links the
/// playback stream to the virtual source by name, so no id is needed.
fn build_mic_streams(state: &Rc<RefCell<State>>) {
    let Some(core) = CORE.with(|c| c.borrow().clone()) else {
        return;
    };
    let mut s = state.borrow_mut();
    if !s.mic_config.enabled {
        return;
    }
    // Resolve "follow default" to the actual hardware source at build
    // time — the capture must be pinned (and must never point at our own
    // virtual mic, or the chain would eat its own output).
    let mic_target = s.mic_config.input_device.clone().or_else(|| {
        s.default_source_name
            .clone()
            .filter(|name| name != MIC_NODE)
    });
    let Some(levels) = s.levels.clone() else { return };
    match MicStreams::new(&core, &s.mic_config, mic_target.as_deref(), levels) {
        Ok(streams) => {
            s.mic_links.clear();
            s.mic_streams = Some(streams);
        }
        Err(e) => eprintln!("sink: mic chain failed: {e}"),
    }
    drop(s);
    ensure_mic_links(state);
}

/// Link the mic playback stream's output ports into the virtual mic.
/// Called whenever ports appear; idempotent.
fn ensure_mic_links(state: &Rc<RefCell<State>>) {
    let Some(core) = CORE.with(|c| c.borrow().clone()) else {
        return;
    };
    let mut s = state.borrow_mut();
    let (Some(playback_id), Some(mic_node)) = (
        s.mic_playback_node(),
        s.node_by_name(MIC_NODE).map(|n| n.id),
    ) else {
        return;
    };
    let pairs = desired_pairs(&s, playback_id, mic_node);
    let current: Vec<(u32, u32)> = s.mic_links.iter().map(|(o, i, _)| (*o, *i)).collect();
    if current == pairs || pairs.is_empty() {
        return;
    }
    s.mic_links.clear();
    s.mic_links = create_links(&core, "mic", playback_id, mic_node, &pairs);
}

/// Compute monitor→input port pairs from `channel_id`'s output ports to
/// `target_id`'s input ports. Pairs by audio.channel where possible, with
/// an index-wrap fallback for mono/odd channel maps.
fn desired_pairs(s: &State, channel_id: u32, target_id: u32) -> Vec<(u32, u32)> {
    if channel_id == target_id {
        return Vec::new();
    }
    let mut monitors: Vec<&PortEntry> = s
        .ports
        .values()
        .filter(|p| p.node_id == channel_id && p.direction == "out")
        .collect();
    let mut inputs: Vec<&PortEntry> = s
        .ports
        .values()
        .filter(|p| p.node_id == target_id && p.direction == "in")
        .collect();
    monitors.sort_by_key(|p| p.id);
    inputs.sort_by_key(|p| p.id);
    if monitors.is_empty() || inputs.is_empty() {
        return Vec::new();
    }
    // Mono source into a multi-channel target: fan out to every input
    // (e.g. listening to the mic — both ears, not just FL).
    if monitors.len() == 1 && inputs.len() > 1 {
        let m = monitors[0];
        return inputs.iter().map(|p| (m.id, p.id)).collect();
    }
    monitors
        .iter()
        .enumerate()
        .filter_map(|(i, m)| {
            let by_channel = m.channel.as_ref().and_then(|ch| {
                inputs
                    .iter()
                    .find(|p| p.channel.as_ref() == Some(ch))
                    .copied()
            });
            let input = by_channel.or_else(|| inputs.get(i % inputs.len()).copied())?;
            Some((m.id, input.id))
        })
        .collect()
}

/// Highest-`priority.session` non-virtual sink from `(id, node.name, priority)`
/// candidates. Pure (plain tuples) so the failover choice is unit-testable,
/// and it reuses WirePlumber's own scoring so Sink's fallback matches the
/// device the OS would pick, consistently across distros.
fn pick_fallback_sink<'a>(candidates: impl Iterator<Item = (u32, &'a str, i64)>) -> Option<u32> {
    candidates
        .filter(|(_, name, _)| !is_virtual_sink(name))
        .max_by_key(|&(_, _, priority)| priority)
        .map(|(id, _, _)| id)
}

/// The real output sink to fall back to when a follow-default channel's
/// default has no live node — e.g. the device was unplugged and WirePlumber
/// hasn't reassigned the default. Without it such a channel gets no links and
/// goes silent (the field-reported "no audio on speakers when headset off").
fn fallback_sink(s: &State) -> Option<u32> {
    pick_fallback_sink(
        s.nodes
            .values()
            .filter(|n| n.media_class == SINK_CLASS)
            .map(|n| {
                (
                    n.id,
                    n.props.get("node.name").map(String::as_str).unwrap_or(""),
                    n.props
                        .get("priority.session")
                        .and_then(|p| p.parse::<i64>().ok())
                        .unwrap_or(0),
                )
            }),
    )
}

/// Which device a channel routes to. `explicit_id` is the pinned device's node
/// id when it's set *and* present; `pinned` is whether a device is pinned at
/// all; `strict` is failover-off. Follow-default and pinned-but-gone channels
/// take the default, then - only when failover is on - the best available
/// sink; in strict mode a gone device resolves to nothing (silence) rather than
/// jumping elsewhere. Pure, so the whole matrix is unit-testable.
fn resolve_target(
    explicit_id: Option<u32>,
    pinned: bool,
    strict: bool,
    default_id: Option<u32>,
    fallback: Option<u32>,
) -> Option<u32> {
    match explicit_id {
        Some(id) => Some(id),
        None if pinned && strict => None,
        None if strict => default_id,
        None => default_id.or(fallback),
    }
}

/// Create link objects for `pairs` between two nodes; returns the proxies.
fn create_links(
    core: &CoreRc,
    sink_name: &str,
    out_node: u32,
    in_node: u32,
    pairs: &[(u32, u32)],
) -> LinkSet {
    let mut created = Vec::new();
    for (monitor_port, input_port) in pairs {
        match core.create_object::<pw::link::Link>(
            "link-factory",
            &pw::properties::properties! {
                "link.output.node" => out_node.to_string(),
                "link.output.port" => monitor_port.to_string(),
                "link.input.node" => in_node.to_string(),
                "link.input.port" => input_port.to_string(),
            },
        ) {
            Ok(link) => created.push((*monitor_port, *input_port, link)),
            Err(e) => eprintln!("sink: link {sink_name} failed: {e}"),
        }
    }
    created
}

/// Reconcile loopback links for every virtual channel:
/// - monitor → chosen output device (or the system default when unset /
///   the chosen device is gone — automatic failover)
/// - monitor → Stream Mix source (Phase 5, for OBS capture)
///
/// Idempotent — existing correct links are left untouched.
fn ensure_all_links(state: &Rc<RefCell<State>>) {
    let Some(core) = CORE.with(|c| c.borrow().clone()) else {
        return;
    };
    let mut s = state.borrow_mut();
    // One name→id snapshot per reconcile instead of a linear node scan per
    // lookup (this runs on every relevant registry event).
    let node_ids: HashMap<String, u32> = s
        .nodes
        .values()
        .filter_map(|n| n.props.get("node.name").map(|name| (name.clone(), n.id)))
        .collect();
    // Live bus nodes: (bus name, node id).
    let bus_ids: Vec<(String, u32)> = s
        .bus_members
        .keys()
        .filter_map(|bus| node_ids.get(bus).map(|id| (bus.clone(), *id)))
        .collect();

    // Live channel set: every virtual sink we created or adopted.
    let channel_names: Vec<String> = s
        .owned_sinks
        .keys()
        .chain(s.adopted_sinks.keys())
        .cloned()
        .collect();

    // Where follow-default channels go when their default has no live node
    // (unplugged, WirePlumber slow/unwilling to reassign): the best available
    // real sink, so audio fails over instead of dropping to silence.
    let fallback = fallback_sink(&s);
    // Forget resolved targets for channels that no longer exist.
    s.channel_targets.retain(|name, _| channel_names.contains(name));

    // The link plan for every live EQ insert, rebuilt from scratch each
    // pass — the link police destroys anything an EQ playback node feeds
    // that isn't in here.
    let mut eq_targets: HashMap<u32, std::collections::HashSet<u32>> = HashMap::new();

    for sink_name in &channel_names {
        let sink_name = sink_name.as_str();
        let channel_id = match node_ids.get(sink_name) {
            Some(id) => *id,
            None => continue,
        };
        // With a live EQ insert, every outgoing link (device, buses,
        // monitor) re-sources from its playback node — one coherent source,
        // so all listeners hear the same (EQ'd, equally delayed) audio.
        let source_id = resolve_source(s.eq_playback_node(sink_name), channel_id);

        // ---- output device links ----
        let explicit = s.channel_outputs.get(sink_name).cloned().flatten();
        let pinned = explicit.is_some();
        let explicit_id = explicit.as_deref().and_then(|name| node_ids.get(name).copied());
        let strict = s.channel_strict.contains(sink_name);
        let default_id = s
            .default_sink_name
            .as_ref()
            .and_then(|name| node_ids.get(name))
            .copied();
        let target_id = resolve_target(explicit_id, pinned, strict, default_id, fallback);
        // Record where this channel resolves to (even when the link set is
        // unchanged) so the UI reflects the live target, including failover.
        match target_id {
            Some(t) => {
                s.channel_targets.insert(sink_name.to_string(), t);
            }
            None => {
                s.channel_targets.remove(sink_name);
            }
        }
        if let (Some(t), true) = (target_id, source_id != channel_id) {
            eq_targets.entry(source_id).or_default().insert(t);
        }
        let pairs = target_id
            .map(|t| desired_pairs(&s, source_id, t))
            .unwrap_or_default();
        let current: Vec<(u32, u32)> = s
            .channel_links
            .get(sink_name)
            .map(|links| links.iter().map(|(o, i, _)| (*o, *i)).collect())
            .unwrap_or_default();
        if current != pairs {
            s.channel_links.remove(sink_name);
            if let Some(in_node) = pairs
                .first()
                .and_then(|(_, input)| s.ports.get(input).map(|p| p.node_id))
            {
                let created = create_links(&core, sink_name, source_id, in_node, &pairs);
                if !created.is_empty() {
                    s.channel_links.insert(sink_name.to_string(), created);
                }
            }
        }

        // ---- mix bus links (one set per bus, membership-gated) ----
        for (bus_name, bus_id) in &bus_ids {
            let included = s
                .bus_members
                .get(bus_name)
                .is_some_and(|members| members.contains(sink_name));
            if included && source_id != channel_id {
                eq_targets.entry(source_id).or_default().insert(*bus_id);
            }
            let pairs = if included {
                desired_pairs(&s, source_id, *bus_id)
            } else {
                Vec::new()
            };
            let key = (bus_name.clone(), sink_name.to_string());
            let current: Vec<(u32, u32)> = s
                .bus_links
                .get(&key)
                .map(|links| links.iter().map(|(o, i, _)| (*o, *i)).collect())
                .unwrap_or_default();
            if current != pairs {
                s.bus_links.remove(&key);
                if !pairs.is_empty() {
                    let created = create_links(&core, sink_name, source_id, *bus_id, &pairs);
                    if !created.is_empty() {
                        s.bus_links.insert(key, created);
                    }
                }
            }
        }
    }

    // ---- monitor links (listen on the default output, session scoped) ----
    let default_id = s
        .default_sink_name
        .as_ref()
        .and_then(|name| node_ids.get(name))
        .copied();
    let monitored: Vec<String> = s.monitored.iter().cloned().collect();
    for name in monitored {
        // Monitoring an EQ'd channel listens to the insert's output — the
        // same audio its device/buses hear.
        let node_id = node_ids
            .get(&name)
            .copied()
            .map(|id| resolve_source(s.eq_playback_node(&name), id));
        if let (Some(node), Some(default)) = (node_id, default_id) {
            if node_ids.get(&name).copied() != Some(node) {
                eq_targets.entry(node).or_default().insert(default);
            }
        }
        let mut pairs = match (node_id, default_id) {
            (Some(node), Some(default)) => desired_pairs(&s, node, default),
            _ => Vec::new(),
        };
        // A channel already playing to the default output needs no extra
        // links (and duplicates would fail) — monitoring is a no-op there.
        if let Some(existing) = s.channel_links.get(&name) {
            let existing_pairs: Vec<(u32, u32)> =
                existing.iter().map(|(o, i, _)| (*o, *i)).collect();
            if existing_pairs == pairs {
                pairs = Vec::new();
            }
        }
        let current: Vec<(u32, u32)> = s
            .monitor_links
            .get(&name)
            .map(|links| links.iter().map(|(o, i, _)| (*o, *i)).collect())
            .unwrap_or_default();
        if current != pairs {
            s.monitor_links.remove(&name);
            if !pairs.is_empty() {
                if let (Some(node), Some(default)) = (node_id, default_id) {
                    let created = create_links(&core, &name, node, default, &pairs);
                    if !created.is_empty() {
                        s.monitor_links.insert(name, created);
                    }
                }
            }
        }
    }

    // Publish the EQ link plan for the police (see on_global's Link arm).
    s.eq_desired_targets = eq_targets;
}

/// The three virtual node shapes we own (kind 0=channel sink, 1=mix bus,
/// 2=virtual mic). The heal path mirrors the create handlers with this.
fn create_node_object(
    core: &CoreRc,
    name: &str,
    label: &str,
    kind: u8,
) -> Result<Node, pw::Error> {
    let class = if kind == 0 { SINK_CLASS } else { VIRTUAL_SOURCE_CLASS };
    let position = if kind == 2 { "[ MONO ]" } else { "[ FL FR ]" };
    let mut props = pw::properties::properties! {
        "factory.name" => "support.null-audio-sink",
        "node.name" => name,
        "node.description" => label,
        "media.class" => class,
        "audio.position" => position,
    };
    if kind == 0 {
        props.insert("monitor.channel-volumes", "true");
    }
    core.create_object::<Node>("adapter", &props)
}

fn handle_cmd(state: &Rc<RefCell<State>>, registry: &RegistryRc, cmd: Cmd) {
    match cmd {
        Cmd::CreateSink { name, label, reply } => {
            let mut s = state.borrow_mut();
            if s.node_by_name(&name).is_some() {
                // Already exists (e.g. leftover from a previous run) — the
                // registry handler has adopted it. Still ours to keep alive.
                s.desired.insert(name, (label, 0));
                let _ = reply.send(Ok(()));
                return;
            }
            if !is_virtual_sink(&name) {
                let _ = reply.send(Err(SinkError::UnknownSink(name)));
                return;
            }
            let Some(core) = CORE.with(|c| c.borrow().clone()) else {
                let _ = reply.send(Err(SinkError::Config(
                    "sink creation requires a live core".into(),
                )));
                return;
            };
            match core.create_object::<Node>(
                "adapter",
                &pw::properties::properties! {
                    "factory.name" => "support.null-audio-sink",
                    "node.name" => name.as_str(),
                    "node.description" => label.as_str(),
                    "media.class" => SINK_CLASS,
                    "audio.position" => "[ FL FR ]",
                    "monitor.channel-volumes" => "true",
                },
            ) {
                // The created proxy must be kept alive until teardown. The
                // reply fires when the global appears in the registry.
                Ok(proxy) => {
                    s.owned_sinks.insert(name.clone(), proxy);
                    s.desired.insert(name.clone(), (label, 0));
                    s.pending_creates.entry(name).or_default().push(reply);
                }
                Err(e) => {
                    let _ = reply.send(Err(SinkError::Config(format!("create sink: {e}"))));
                }
            }
        }
        Cmd::DestroySink { name, reply } => {
            let mut s = state.borrow_mut();
            s.desired.remove(&name);
            s.meters.remove(&name);
            // Drop the EQ insert before the sink proxy goes away so the
            // capture stream's target doesn't vanish under it mid-teardown.
            s.eq_streams.remove(&name);
            s.eq_configs.remove(&name);
            s.channel_links.remove(&name);
            s.bus_links.retain(|(_, ch), _| ch != &name);
            s.channel_outputs.remove(&name);
            if let Some(levels) = &s.levels {
                levels.release(&name);
            }
            if let Some(proxy) = s.owned_sinks.remove(&name) {
                match CORE.with(|c| c.borrow().clone()) {
                    Some(core) => {
                        let _ = core.destroy_object(proxy);
                        let _ = reply.send(Ok(()));
                    }
                    None => {
                        let _ = reply.send(Err(SinkError::Config("core is gone".into())));
                    }
                }
            } else if let Some(id) = s.adopted_sinks.remove(&name) {
                let _ = registry.destroy_global(id);
                let _ = reply.send(Ok(()));
            } else {
                // Nothing to destroy — idempotent success.
                let _ = reply.send(Ok(()));
            }
        }
        Cmd::ListStreams { reply } => {
            let s = state.borrow();
            let streams = s
                .nodes
                .values()
                .filter(|n| n.media_class == STREAM_CLASS)
                .map(|n| {
                    let (app_name, match_prop, match_value) =
                        crate::audio::types::resolve_identity(|key| n.props.get(key).cloned());
                    AppStream {
                        index: n.id,
                        app_name,
                        match_prop,
                        match_value,
                        alias: None,
                        icon_name: n.props.get("application.icon-name").cloned(),
                        icon_path: None,
                        pid: n
                            .props
                            .get("application.process.id")
                            .and_then(|v| v.parse().ok()),
                        assigned_sink: s
                            .sink_of_stream(n.id)
                            .and_then(|sink| sink.props.get("node.name"))
                            .filter(|name| is_virtual_sink(name))
                            .cloned(),
                        volume_percent: n.volume_percent,
                        muted: n.muted,
                        active: n.active,
                    }
                })
                .collect();
            let _ = reply.send(Ok(streams));
        }
        Cmd::ListOutputs { reply } => {
            let s = state.borrow();
            let outputs = s
                .nodes
                .values()
                .filter(|n| {
                    n.media_class == SINK_CLASS
                        && !n
                            .props
                            .get("node.name")
                            .is_some_and(|name| is_virtual_sink(name))
                })
                .map(|n| OutputDevice {
                    index: n.id,
                    name: n.props.get("node.name").cloned().unwrap_or_default(),
                    description: n
                        .props
                        .get("node.description")
                        .or_else(|| n.props.get("node.nick"))
                        .cloned()
                        .unwrap_or_default(),
                })
                .collect();
            let _ = reply.send(Ok(outputs));
        }
        Cmd::ResolvedOutputs { reply } => {
            let s = state.borrow();
            let resolved = s
                .owned_sinks
                .keys()
                .chain(s.adopted_sinks.keys())
                .map(|name| {
                    let device = s
                        .channel_targets
                        .get(name)
                        .and_then(|id| s.nodes.get(id))
                        .and_then(|n| n.props.get("node.name").cloned());
                    (name.clone(), device)
                })
                .collect();
            let _ = reply.send(Ok(resolved));
        }
        Cmd::SetNodeVolumeByName { name, percent, reply } => {
            let s = state.borrow();
            let _ = reply.send(set_props(s.node_by_name(&name), Some(percent), None));
        }
        Cmd::SetNodeMuteByName { name, muted, reply } => {
            let s = state.borrow();
            let _ = reply.send(set_props(s.node_by_name(&name), None, Some(muted)));
        }
        Cmd::SetNodeVolumeById { id, percent, reply } => {
            let s = state.borrow();
            let _ = reply.send(set_props(s.nodes.get(&id), Some(percent), None));
        }
        Cmd::CreateBus { name, label, reply } => {
            let mut s = state.borrow_mut();
            if s.bus_sources.contains_key(&name) || s.node_by_name(&name).is_some() {
                s.desired.insert(name, (label, 1)); // adopted — keep alive
                let _ = reply.send(Ok(()));
                return;
            }
            let Some(core) = CORE.with(|c| c.borrow().clone()) else {
                let _ = reply.send(Err(SinkError::Config("core is gone".into())));
                return;
            };
            match core.create_object::<Node>(
                "adapter",
                &pw::properties::properties! {
                    "factory.name" => "support.null-audio-sink",
                    "node.name" => name.as_str(),
                    "node.description" => label.as_str(),
                    "media.class" => VIRTUAL_SOURCE_CLASS,
                    "audio.position" => "[ FL FR ]",
                },
            ) {
                Ok(proxy) => {
                    s.desired.insert(name.clone(), (label, 1));
                    s.bus_sources.insert(name, proxy);
                    let _ = reply.send(Ok(()));
                }
                Err(e) => {
                    let _ = reply.send(Err(SinkError::Config(format!("create bus: {e}"))));
                }
            }
        }
        Cmd::DestroyBus { name, reply } => {
            let mut s = state.borrow_mut();
            s.desired.remove(&name);
            s.meters.remove(&name);
            s.bus_members.remove(&name);
            s.bus_links.retain(|(bus, _), _| bus != &name);
            if let Some(levels) = &s.levels {
                levels.release(&name);
            }
            if let Some(proxy) = s.bus_sources.remove(&name) {
                if let Some(core) = CORE.with(|c| c.borrow().clone()) {
                    let _ = core.destroy_object(proxy);
                }
            }
            let _ = reply.send(Ok(()));
        }
        Cmd::SetBusMembers { name, channels, reply } => {
            state
                .borrow_mut()
                .bus_members
                .insert(name, channels.into_iter().collect());
            ensure_all_links(state);
            let _ = reply.send(Ok(()));
        }
        Cmd::SetMonitor { name, enabled, reply } => {
            {
                let mut s = state.borrow_mut();
                if enabled {
                    s.monitored.insert(name);
                } else {
                    s.monitored.remove(&name);
                    s.monitor_links.remove(&name);
                }
            }
            ensure_all_links(state);
            let _ = reply.send(Ok(()));
        }
        Cmd::SetChannelEq { sink_name, config, reply } => {
            if !is_virtual_sink(&sink_name) {
                let _ = reply.send(Err(SinkError::UnknownSink(sink_name)));
                return;
            }
            let (needs_create, needs_destroy) = {
                let mut s = state.borrow_mut();
                s.eq_configs.insert(sink_name.clone(), config.clone());
                // Live re-tune: band edits reach the RT thread through the
                // params atomics, no relink and no audible gap.
                if let Some(handle) = s.eq_streams.get(&sink_name) {
                    handle.params.apply(&config);
                }
                let live = s.eq_streams.contains_key(&sink_name);
                (config.enabled && !live, !config.enabled && live)
            };
            if needs_destroy {
                state.borrow_mut().eq_streams.remove(&sink_name);
            } else if needs_create {
                let sink_id = state
                    .borrow()
                    .node_by_name(&sink_name)
                    .map(|n| n.id);
                if let Some(sink_id) = sink_id {
                    let Some(core) = CORE.with(|c| c.borrow().clone()) else {
                        let _ = reply.send(Err(SinkError::Config(
                            "eq chain requires a live core".into(),
                        )));
                        return;
                    };
                    match EqChainHandle::new(&core, &sink_name, sink_id, &config) {
                        Ok(handle) => {
                            state.borrow_mut().eq_streams.insert(sink_name.clone(), handle);
                        }
                        Err(e) => {
                            let _ = reply.send(Err(e));
                            return;
                        }
                    }
                }
                // Sink not live yet (e.g. mid-profile-load): the on_node
                // hook builds the chain from eq_configs when it appears.
            }
            // Re-source the channel's links from/to the insert.
            ensure_all_links(state);
            let _ = reply.send(Ok(()));
        }
        Cmd::SetMicConfig { config, reply } => {
            let (needs_create, needs_destroy, needs_rebuild, source_exists, orphaned) = {
                let mut s = state.borrow_mut();
                let prev = s.mic_config.clone();
                s.mic_config = config.clone();

                // Live-tunable params apply without a rebuild.
                if let Some(streams) = &s.mic_streams {
                    streams.params.apply(&config);
                }

                // Renaming the published mic recreates the node so other
                // apps see the new description immediately.
                let needs_recreate = config.enabled
                    && s.mic_source.is_some()
                    && prev.output_label != config.output_label;
                let mut orphaned: Vec<u32> = Vec::new();
                if needs_recreate {
                    // Remember who was capturing the mic (Discord, OBS …) —
                    // destroying the node drops them onto the fallback
                    // source, and they'd silently stay there.
                    if let Some(mic) = s.node_by_name(MIC_NODE) {
                        let mic_id = mic.id;
                        orphaned = s
                            .links
                            .values()
                            .filter(|(out, _)| *out == mic_id)
                            .map(|(_, input)| *input)
                            // Tracked nodes here are devices (monitor
                            // targets) — foreign capture streams aren't in
                            // the mirror.
                            .filter(|input| !s.nodes.contains_key(input))
                            .collect();
                    }
                    s.mic_streams = None;
                    s.mic_links.clear();
                    if let Some(proxy) = s.mic_source.take() {
                        // Our own destroy — the heal path should expect this
                        // removal rather than treat it as external and race a
                        // second recreate.
                        s.mic_expected_removals += 1;
                        if let Some(core) = CORE.with(|c| c.borrow().clone()) {
                            let _ = core.destroy_object(proxy);
                        }
                    }
                }

                let needs_create = config.enabled && s.mic_source.is_none();
                let needs_destroy = !config.enabled && s.mic_source.is_some();
                let needs_rebuild = config.enabled
                    && s.mic_streams.is_some()
                    && prev.input_device != config.input_device;
                let source_exists = s.node_by_name(MIC_NODE).is_some();
                (needs_create, needs_destroy, needs_rebuild, source_exists, orphaned)
            };

            if needs_destroy {
                let mut s = state.borrow_mut();
                s.desired.remove(MIC_NODE);
                s.mic_streams = None;
                s.mic_links.clear();
                if let Some(proxy) = s.mic_source.take() {
                    if let Some(core) = CORE.with(|c| c.borrow().clone()) {
                        let _ = core.destroy_object(proxy);
                    }
                }
                let _ = reply.send(Ok(()));
                return;
            }

            if needs_create {
                let Some(core) = CORE.with(|c| c.borrow().clone()) else {
                    let _ = reply.send(Err(SinkError::Config("core is gone".into())));
                    return;
                };
                match core.create_object::<Node>(
                    "adapter",
                    &pw::properties::properties! {
                        "factory.name" => "support.null-audio-sink",
                        "node.name" => MIC_NODE,
                        "node.description" => config.output_label.as_str(),
                        "media.class" => VIRTUAL_SOURCE_CLASS,
                        "audio.position" => "[ MONO ]",
                    },
                ) {
                    Ok(proxy) => {
                        let mut s = state.borrow_mut();
                        s.mic_source = Some(proxy);
                        s.desired
                            .insert(MIC_NODE.to_string(), (config.output_label.clone(), 2));
                        // Re-point streams that were capturing the old node
                        // (target.object by name survives the recreation —
                        // the session manager re-attaches them when the new
                        // global appears). Type stays None deliberately:
                        // that's what `pw-metadata <id> target.object <name>`
                        // sets, and WirePlumber matches the value against
                        // serials first, node names second, regardless of
                        // the annotation. Spa:Id (used for serial-based
                        // moves elsewhere) would be wrong for a name.
                        if let Some(meta) = &s.metadata {
                            for id in &orphaned {
                                meta.set_property(*id, "target.object", None, Some(MIC_NODE));
                            }
                        }
                        // Streams attach when the global appears (on_node).
                    }
                    Err(e) => {
                        let _ =
                            reply.send(Err(SinkError::Config(format!("create mic source: {e}"))));
                        return;
                    }
                }
            } else if needs_rebuild {
                state.borrow_mut().mic_streams = None;
                if source_exists {
                    build_mic_streams(state);
                }
            } else if config.enabled && source_exists {
                // Source exists but streams may be missing (earlier failure
                // or config re-applied at startup) — attach if needed.
                let missing = state.borrow().mic_streams.is_none();
                if missing {
                    build_mic_streams(state);
                }
            }
            let _ = reply.send(Ok(()));
        }
        Cmd::ListInputs { reply } => {
            let s = state.borrow();
            let inputs = s
                .nodes
                .values()
                .filter(|n| {
                    let name = n.props.get("node.name").map(String::as_str);
                    n.media_class == SOURCE_CLASS
                        || (n.media_class == VIRTUAL_SOURCE_CLASS
                            && name != Some(MIC_NODE)
                            && !name.is_some_and(is_bus_name))
                })
                .map(|n| OutputDevice {
                    index: n.id,
                    name: n.props.get("node.name").cloned().unwrap_or_default(),
                    description: n
                        .props
                        .get("node.description")
                        .or_else(|| n.props.get("node.nick"))
                        .cloned()
                        .unwrap_or_default(),
                })
                .collect();
            let _ = reply.send(Ok(inputs));
        }
        Cmd::GetDefaults { reply } => {
            let s = state.borrow();
            let _ = reply.send(Ok((
                s.default_sink_name.clone(),
                s.default_source_name.clone(),
            )));
        }
        Cmd::SetDefault { input, name, reply } => {
            let s = state.borrow();
            let Some(metadata) = s.metadata.as_ref() else {
                let _ = reply.send(Err(SinkError::Config(
                    "no default metadata object (is WirePlumber running?)".into(),
                )));
                return;
            };
            // The same mechanism wpctl uses: WirePlumber watches the
            // configured keys and applies + persists the choice.
            let key = if input {
                "default.configured.audio.source"
            } else {
                "default.configured.audio.sink"
            };
            // Build the Spa:String:JSON value with serde so backslashes,
            // quotes and control chars are all escaped - a hand-rolled
            // format! that only escaped `"` let a name ending in `\` break
            // out of the quoted string and inject metadata keys (TD-018).
            let value = serde_json::json!({ "name": name }).to_string();
            metadata.set_property(0, key, Some("Spa:String:JSON"), Some(&value));
            let _ = reply.send(Ok(()));
        }
        Cmd::SetChannelOutput { sink_name, output_name, reply } => {
            if !is_virtual_sink(&sink_name) {
                let _ = reply.send(Err(SinkError::UnknownSink(sink_name)));
                return;
            }
            state
                .borrow_mut()
                .channel_outputs
                .insert(sink_name, output_name);
            ensure_all_links(state);
            let _ = reply.send(Ok(()));
        }
        Cmd::SetChannelFailover { sink_name, enabled, reply } => {
            if !is_virtual_sink(&sink_name) {
                let _ = reply.send(Err(SinkError::UnknownSink(sink_name)));
                return;
            }
            {
                let mut s = state.borrow_mut();
                if enabled {
                    s.channel_strict.remove(&sink_name);
                } else {
                    s.channel_strict.insert(sink_name);
                }
            }
            ensure_all_links(state);
            let _ = reply.send(Ok(()));
        }
        Cmd::MoveStream { id, sink_name, reply } => {
            let s = state.borrow();
            let Some(metadata) = s.metadata.as_ref() else {
                let _ = reply.send(Err(SinkError::Config(
                    "no default metadata object (is WirePlumber running?)".into(),
                )));
                return;
            };
            // Empty sink name = back to the default device.
            let target = if sink_name.is_empty() {
                s.default_sink_name.clone()
            } else {
                Some(sink_name.clone())
            };
            let serial = target
                .as_deref()
                .and_then(|name| s.node_by_name(name))
                .and_then(|n| n.serial);
            match serial {
                Some(serial) => {
                    metadata.set_property(
                        id,
                        "target.object",
                        Some("Spa:Id"),
                        Some(&serial.to_string()),
                    );
                    // Clear any stale low-level target left by other tools.
                    metadata.set_property(id, "target.node", None, None);
                    let _ = reply.send(Ok(()));
                }
                None => {
                    let _ = reply.send(Err(SinkError::UnknownSink(
                        target.unwrap_or_else(|| "<default>".into()),
                    )));
                }
            }
        }
    }
}

fn set_props(
    entry: Option<&NodeEntry>,
    volume_percent: Option<u8>,
    mute: Option<bool>,
) -> Result<(), SinkError> {
    let Some(entry) = entry else {
        return Err(SinkError::UnknownSink("node not found".into()));
    };
    let volume = volume_percent.map(|p| (pods::percent_to_linear(p), entry.channels));
    let bytes = pods::props_pod_bytes(volume, mute)?;
    let pod = pw::spa::pod::Pod::from_bytes(&bytes)
        .ok_or_else(|| SinkError::Config("constructed an invalid pod".into()))?;
    entry
        .proxy
        .set_param(pw::spa::param::ParamType::Props, 0, pod);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn port(id: u32, node_id: u32, dir: &str, channel: Option<&str>) -> PortEntry {
        PortEntry {
            id,
            node_id,
            direction: dir.to_string(),
            channel: channel.map(str::to_string),
        }
    }

    #[test]
    fn resolve_source_prefers_live_eq_playback() {
        assert_eq!(resolve_source(Some(77), 10), 77);
    }

    #[test]
    fn resolve_source_falls_back_to_channel() {
        assert_eq!(resolve_source(None, 10), 10);
    }

    #[test]
    fn desired_pairs_matches_stereo_by_channel_not_index() {
        let mut s = State::default();
        // Monitor FL/FR on node 10, inputs FL/FR on node 20 with ids ordered
        // so a naive index pairing would cross the channels.
        s.ports.insert(1, port(1, 10, "out", Some("FL")));
        s.ports.insert(2, port(2, 10, "out", Some("FR")));
        s.ports.insert(3, port(3, 20, "in", Some("FR")));
        s.ports.insert(4, port(4, 20, "in", Some("FL")));
        let mut pairs = desired_pairs(&s, 10, 20);
        pairs.sort_unstable();
        assert_eq!(pairs, vec![(1, 4), (2, 3)]);
    }

    #[test]
    fn desired_pairs_fans_mono_source_to_every_input() {
        let mut s = State::default();
        s.ports.insert(1, port(1, 10, "out", Some("MONO")));
        s.ports.insert(2, port(2, 20, "in", Some("FL")));
        s.ports.insert(3, port(3, 20, "in", Some("FR")));
        let mut pairs = desired_pairs(&s, 10, 20);
        pairs.sort_unstable();
        assert_eq!(pairs, vec![(1, 2), (1, 3)]);
    }

    #[test]
    fn desired_pairs_empty_for_self_or_missing_ports() {
        let mut s = State::default();
        s.ports.insert(1, port(1, 10, "out", Some("FL")));
        assert!(desired_pairs(&s, 10, 10).is_empty(), "same node");
        assert!(desired_pairs(&s, 10, 20).is_empty(), "target has no inputs");
    }

    #[test]
    fn fallback_picks_highest_priority_real_sink() {
        let candidates = [
            (1u32, "sink_game", 10_000i64), // virtual — never a fallback
            (2, "alsa_output.hdmi", 500),
            (3, "alsa_output.analog", 900),
            (4, "alsa_output.usb", 700),
        ];
        assert_eq!(pick_fallback_sink(candidates.into_iter()), Some(3));
    }

    #[test]
    fn resolve_target_covers_the_failover_matrix() {
        // Pinned and present -> that device, failover on or off.
        assert_eq!(resolve_target(Some(7), true, false, Some(1), Some(2)), Some(7));
        assert_eq!(resolve_target(Some(7), true, true, Some(1), Some(2)), Some(7));
        // Follow-default, failover on -> default, else the fallback sink.
        assert_eq!(resolve_target(None, false, false, Some(1), Some(2)), Some(1));
        assert_eq!(resolve_target(None, false, false, None, Some(2)), Some(2));
        // Follow-default, failover off -> default only; silent when it's gone.
        assert_eq!(resolve_target(None, false, true, Some(1), Some(2)), Some(1));
        assert_eq!(resolve_target(None, false, true, None, Some(2)), None);
        // Pinned but gone, failover on -> default then fallback.
        assert_eq!(resolve_target(None, true, false, Some(1), Some(2)), Some(1));
        assert_eq!(resolve_target(None, true, false, None, Some(2)), Some(2));
        // Pinned but gone, failover off -> silence, never another device.
        assert_eq!(resolve_target(None, true, true, Some(1), Some(2)), None);
    }

    #[test]
    fn fallback_is_none_when_only_virtual_channel_sinks_exist() {
        // Channel sinks are virtual and must never be a fallback target.
        // (sink_mic/sink_stream are sources, excluded by media_class before
        // reaching here — so they're not exercised at this layer.)
        let candidates = [(1u32, "sink_game", 0i64), (2, "sink_chat", 0)];
        assert_eq!(pick_fallback_sink(candidates.into_iter()), None);
    }
}
