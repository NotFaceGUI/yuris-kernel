//! fault dump:两份文件(JSONL + timeline.json)的写出/读取与 JSON 投影。
//!
//! 从 `fault.rs` 纯搬运(2026-10-23);行为不变。

use super::replay::FaultReplayReport;
use super::*;

/// dump 的两份文件路径。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaultDumpPaths {
    pub trace: PathBuf,
    pub manifest: PathBuf,
}

impl FaultDumpPaths {
    pub fn for_trace(path: impl AsRef<Path>) -> Self {
        let trace = path.as_ref().to_path_buf();
        Self {
            manifest: sidecar_path(&trace),
            trace,
        }
    }
}

fn sidecar_path(trace: &Path) -> PathBuf {
    let mut name = trace
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| "fault".into());
    name.push(".timeline.json");
    match trace.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
        _ => PathBuf::from(name),
    }
}

/// 读取并校验过的故障快照。
#[derive(Clone, Debug)]
pub struct FaultDump {
    pub path: PathBuf,
    pub log: TraceLog,
    pub header: HeaderPayload,
    pub manifest: FaultManifest,
}

impl FaultDump {
    pub fn paths(path: impl AsRef<Path>) -> FaultDumpPaths {
        FaultDumpPaths::for_trace(path)
    }

    /// 读取 `<path>` + `<path>.timeline.json`,校验 trace 摘要与 header 一致性。
    pub fn read(path: impl AsRef<Path>) -> Result<Self, Diag> {
        let path = path.as_ref().to_path_buf();
        let paths = FaultDumpPaths::for_trace(&path);
        let text = std::fs::read_to_string(&paths.trace)
            .map_err(|e| error::io(format!("read {}: {e}", paths.trace.display())))?;
        let manifest_text = std::fs::read_to_string(&paths.manifest)
            .map_err(|e| error::io(format!("read {}: {e}", paths.manifest.display())))?;
        let log = TraceLog::parse(&text)?;
        let header = log
            .header()
            .cloned()
            .ok_or_else(|| error::schema(1, "fault dump trace is missing the header record"))?;
        let manifest = FaultManifest::from_json(&manifest_text)?;
        let actual = sha256_hex(text.as_bytes());
        if actual != manifest.trace_sha256 {
            return Err(error::schema(
                1,
                format!(
                    "fault dump trace_sha256 {} does not match recorded {}",
                    actual, manifest.trace_sha256
                ),
            ));
        }
        if manifest.producer != header.producer {
            return Err(error::schema(
                1,
                "fault dump manifest producer does not match trace header",
            ));
        }
        let timeline = manifest.timeline();
        let expected = crate::event::input_summary_range(&timeline, 1, manifest.last_frame);
        if expected.events != header.input.events || expected.sha256 != header.input.sha256 {
            return Err(error::schema(
                1,
                format!(
                    "fault dump timeline digest {} does not match header input {}",
                    expected.sha256, header.input.sha256
                ),
            ));
        }
        Ok(Self {
            path,
            log,
            header,
            manifest,
        })
    }

    pub fn timeline(&self) -> InputTimeline {
        self.manifest.timeline()
    }

    pub fn fault_frame(&self) -> u64 {
        self.manifest.fault_frame
    }

    /// 按 dump 的 clock 序列离线重演到故障 tick。
    ///
    /// 数据源/argv/宿主路径必须与 dump 的 header 摘要一致(由
    /// [`crate::record::BootSummary::to_config`] 与 `verify_input_range` 校验)。
    pub fn replay(
        &self,
        source: Box<dyn DataSource>,
        aux: Option<Box<dyn DataSource>>,
        argv: Vec<Vec<u8>>,
        paths: yu_rt::vm::HostPaths,
    ) -> Result<FaultReplayReport, Diag> {
        if !self.manifest.replayable() {
            return Err(
                Diag::fatal(error::code::REPLAY, Boundary::Domain).with_detail(format!(
                    "fault dump is not replayable from boot: start_frame {} evicted_ticks {} \
                 timeline_truncated {} clock_truncated {} placeholders {}",
                    self.manifest.start_frame,
                    self.manifest.evicted_ticks,
                    self.manifest.timeline_truncated,
                    self.manifest.clock_truncated,
                    self.manifest
                        .ticks
                        .iter()
                        .filter(|tick| !tick.retained)
                        .count()
                )),
            );
        }
        let timeline = self.timeline();
        crate::replay::verify_input_range(&self.header, &timeline, 1, self.manifest.last_frame)?;
        let mut trace = VmTrace::from_header(&self.header, source, aux, argv, paths)?;
        let tick_count = self
            .manifest
            .fault_tick
            .map(|tick| tick.saturating_add(1))
            .unwrap_or(0);
        crate::replay::verify_clock(&self.header, &clock_ticks(&self.manifest.clock, tick_count))?;
        trace.enable_fault_ring(self.manifest.ring.clone(), Some(self.header.clone()))?;

        let mut frames_replayed = 0u64;
        for index in 0..tick_count {
            let now_ms = clock_at(&self.manifest.clock, index as usize)?;
            let report = match now_ms {
                Some(now) => trace.tick_at(now, &timeline),
                None => trace.tick(&timeline),
            };
            frames_replayed += report.logical_frames as u64;
            let actual = trace.recorder().finish_fault_tick().ok_or_else(|| {
                Diag::fatal(error::code::REPLAY, Boundary::Domain).with_detail(format!(
                    "replay produced no tick {index} (ring was not attached before the tick)"
                ))
            })?;
            let expected = self
                .manifest
                .ticks
                .iter()
                .find(|tick| tick.index == index)
                .ok_or_else(|| {
                    Diag::fatal(error::code::REPLAY, Boundary::Domain).with_detail(format!(
                        "fault dump has no tick {index} (evicted? ring ticks {})",
                        self.manifest.ticks.len()
                    ))
                })?;
            if let Some(detail) = compare_ticks(expected, &actual) {
                return Err(
                    Diag::fatal(error::code::REPLAY, Boundary::Domain).with_detail(format!(
                        "fault replay diverged at tick {index} (fault frame {}): {detail}",
                        self.manifest.fault_frame
                    )),
                );
            }
        }
        Ok(FaultReplayReport {
            ticks_replayed: tick_count,
            frames_replayed,
            fault_tick: self.manifest.fault_tick,
            fault_frame: self.manifest.fault_frame,
        })
    }
}

/// 离线重演结果(零差异才会返回)。
/// 原子写:同目录临时文件 + `rename`(Windows 上是 MOVEFILE_REPLACE_EXISTING)。
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), Diag> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| error::io(format!("create {}: {e}", parent.display())))?;
        }
    }
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "fault".to_string());
    let temp = path.with_file_name(format!(
        ".{name}.tmp{}.{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&temp, bytes)
        .map_err(|e| error::io(format!("write {}: {e}", temp.display())))?;
    std::fs::rename(&temp, path).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        error::io(format!(
            "rename {} -> {}: {e}",
            temp.display(),
            path.display()
        ))
    })
}

impl FaultManifest {
    /// 写 dump:先写伴生文件,最后写 JSONL(提交标记)。
    ///
    /// `trace_text` 必须是 `self.trace_sha256` 对应的文本。
    pub fn write_bundle(&self, paths: &FaultDumpPaths, trace_text: &str) -> Result<(), Diag> {
        let manifest_text = self.to_json_text();
        atomic_write(&paths.manifest, manifest_text.as_bytes())?;
        atomic_write(&paths.trace, trace_text.as_bytes())
    }

    pub fn to_json_text(&self) -> String {
        json::to_text(&self.to_json())
    }

    pub fn from_json(text: &str) -> Result<Self, Diag> {
        let value = json::parse(text).map_err(|detail| {
            Diag::fatal(error::code::PARSE, Boundary::Format)
                .with_detail(format!("fault manifest is not valid JSON: {detail}"))
        })?;
        let obj = json::as_obj(&value).map_err(|detail| {
            Diag::fatal(error::code::PARSE, Boundary::Format).with_detail(detail)
        })?;
        let manifest = parse_manifest(obj)?;
        Ok(manifest)
    }

    pub fn timeline(&self) -> InputTimeline {
        let mut timeline = InputTimeline::new();
        for entry in &self.input.entries {
            timeline.set(entry.frame, entry.to_input());
        }
        timeline
    }

    fn to_json(&self) -> Json {
        Json::Obj(vec![
            (
                "schema".to_string(),
                Json::UInt(FAULT_MANIFEST_SCHEMA as u64),
            ),
            ("kind".to_string(), Json::Str("fault_dump".to_string())),
            (
                "trace_sha256".to_string(),
                Json::Str(self.trace_sha256.clone()),
            ),
            (
                "producer".to_string(),
                Json::Obj(vec![
                    (
                        "kind".to_string(),
                        Json::Str(self.producer.kind.as_str().to_string()),
                    ),
                    ("name".to_string(), Json::Str(self.producer.name.clone())),
                    (
                        "version".to_string(),
                        Json::Str(self.producer.version.clone()),
                    ),
                    (
                        "sample_sha256".to_string(),
                        match &self.producer.sample_sha256 {
                            Some(hash) => Json::Str(hash.clone()),
                            None => Json::Null,
                        },
                    ),
                ]),
            ),
            ("codec".to_string(), Json::Str(self.codec.clone())),
            (
                "fault".to_string(),
                Json::Obj(vec![
                    (
                        "kind".to_string(),
                        Json::Str(self.fault.kind.as_str().to_string()),
                    ),
                    ("reason".to_string(), Json::Str(self.fault.reason.clone())),
                    ("detail".to_string(), Json::Str(self.fault.detail.clone())),
                    (
                        "tick".to_string(),
                        match self.fault_tick {
                            Some(tick) => Json::UInt(tick),
                            None => Json::Null,
                        },
                    ),
                    ("frame".to_string(), Json::UInt(self.fault_frame)),
                ]),
            ),
            ("ring".to_string(), ring_json(&self.ring)),
            (
                "input".to_string(),
                Json::Obj(vec![
                    ("events".to_string(), Json::UInt(self.input.events)),
                    ("sha256".to_string(), Json::Str(self.input.sha256.clone())),
                    ("truncated".to_string(), Json::Bool(self.input.truncated)),
                    (
                        "entries".to_string(),
                        Json::Arr(self.input.entries.iter().map(input_entry_json).collect()),
                    ),
                ]),
            ),
            ("clock".to_string(), clock_json(&self.clock)),
            ("start_frame".to_string(), Json::UInt(self.start_frame)),
            ("last_frame".to_string(), Json::UInt(self.last_frame)),
            ("orphan_events".to_string(), Json::UInt(self.orphan_events)),
            ("vfs_records".to_string(), Json::UInt(self.vfs_records)),
            ("vfs_dropped".to_string(), Json::UInt(self.vfs_dropped)),
            (
                "boot_records_dropped".to_string(),
                Json::UInt(self.boot_records_dropped),
            ),
            (
                "timeline_truncated".to_string(),
                Json::Bool(self.timeline_truncated),
            ),
            (
                "clock_truncated".to_string(),
                Json::Bool(self.clock_truncated),
            ),
            ("evicted_ticks".to_string(), Json::UInt(self.evicted_ticks)),
            (
                "ticks".to_string(),
                Json::Arr(self.ticks.iter().map(tick_json).collect()),
            ),
        ])
    }
}

fn ring_json(ring: &RingConfig) -> Json {
    Json::Obj(vec![
        ("ticks".to_string(), Json::UInt(ring.ticks as u64)),
        (
            "max_tick_bytes".to_string(),
            Json::UInt(ring.max_tick_bytes as u64),
        ),
        (
            "max_total_bytes".to_string(),
            Json::UInt(ring.max_total_bytes as u64),
        ),
        (
            "max_boot_records".to_string(),
            Json::UInt(ring.max_boot_records as u64),
        ),
        (
            "max_diags_per_tick".to_string(),
            Json::UInt(ring.max_diags_per_tick as u64),
        ),
        (
            "max_inputs_per_tick".to_string(),
            Json::UInt(ring.max_inputs_per_tick as u64),
        ),
        (
            "max_display_bytes".to_string(),
            Json::UInt(ring.max_display_bytes as u64),
        ),
        (
            "max_timeline_entries".to_string(),
            Json::UInt(ring.max_timeline_entries as u64),
        ),
        (
            "max_clock_ticks".to_string(),
            Json::UInt(ring.max_clock_ticks as u64),
        ),
    ])
}

fn clock_json(clock: &ClockSummary) -> Json {
    Json::Obj(vec![
        (
            "mode".to_string(),
            Json::Str(clock.mode.as_str().to_string()),
        ),
        (
            "now_ms".to_string(),
            Json::Arr(
                clock
                    .now_ms
                    .iter()
                    .map(|value| match value {
                        Some(now) => Json::UInt(*now as u64),
                        None => Json::Null,
                    })
                    .collect(),
            ),
        ),
    ])
}

fn input_entry_json(entry: &InputEntry) -> Json {
    Json::Obj(vec![
        ("frame".to_string(), Json::UInt(entry.frame)),
        (
            "cursor".to_string(),
            Json::Arr(vec![Json::Int(entry.cursor.0), Json::Int(entry.cursor.1)]),
        ),
        (
            "pressed".to_string(),
            Json::Arr(entry.pressed.iter().cloned().map(Json::Str).collect()),
        ),
        (
            "pulses".to_string(),
            Json::Arr(entry.pulses.iter().cloned().map(Json::Str).collect()),
        ),
    ])
}

fn tick_json(tick: &TickSummary) -> Json {
    Json::Obj(vec![
        ("index".to_string(), Json::UInt(tick.index)),
        ("first_frame".to_string(), Json::UInt(tick.first_frame)),
        ("frames".to_string(), Json::UInt(tick.frames)),
        ("last_frame".to_string(), Json::UInt(tick.last_frame)),
        (
            "now_ms".to_string(),
            match tick.now_ms {
                Some(now) => Json::UInt(now as u64),
                None => Json::Null,
            },
        ),
        ("command_count".to_string(), Json::UInt(tick.command_count)),
        (
            "command_digest".to_string(),
            Json::Str(tick.command_digest.clone()),
        ),
        (
            "event_digest".to_string(),
            Json::Str(tick.event_digest.clone()),
        ),
        (
            "event_counts".to_string(),
            Json::Obj(
                tick.event_counts
                    .iter()
                    .map(|(kind, count)| (kind.clone(), Json::UInt(*count)))
                    .collect(),
            ),
        ),
        (
            "diags".to_string(),
            Json::Arr(tick.diags.iter().map(diag_json).collect()),
        ),
        ("diags_dropped".to_string(), Json::UInt(tick.diags_dropped)),
        (
            "inputs".to_string(),
            Json::Arr(tick.inputs.iter().map(input_entry_json).collect()),
        ),
        (
            "inputs_dropped".to_string(),
            Json::UInt(tick.inputs_dropped),
        ),
        (
            "frame_descriptions".to_string(),
            Json::Arr(
                tick.frames_desc
                    .iter()
                    .map(|entry| {
                        Json::Obj(vec![
                            ("frame".to_string(), Json::UInt(entry.frame)),
                            (
                                "display_sha256".to_string(),
                                Json::Str(entry.display_sha256.clone()),
                            ),
                            (
                                "display_retained".to_string(),
                                Json::Bool(entry.display_retained),
                            ),
                            (
                                "pass_count".to_string(),
                                Json::UInt(entry.pass_count as u64),
                            ),
                            ("op_count".to_string(), Json::UInt(entry.op_count)),
                            (
                                "executable_ops".to_string(),
                                Json::UInt(entry.executable_ops),
                            ),
                            ("skipped_ops".to_string(), Json::UInt(entry.skipped_ops)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "displays_dropped".to_string(),
            Json::UInt(tick.displays_dropped),
        ),
        ("bytes".to_string(), Json::UInt(tick.bytes as u64)),
        ("over_budget".to_string(), Json::Bool(tick.over_budget)),
        ("retained".to_string(), Json::Bool(tick.retained)),
    ])
}

fn diag_json(entry: &DiagEntry) -> Json {
    Json::Obj(vec![
        ("frame".to_string(), Json::UInt(entry.frame)),
        ("code".to_string(), Json::Str(entry.code.clone())),
        ("detail".to_string(), Json::Str(entry.detail.clone())),
        ("unimplemented".to_string(), Json::Bool(entry.unimplemented)),
    ])
}

// ------------------------------------------------------------------ manifest 解析

fn field_error(detail: String) -> Diag {
    Diag::fatal(error::code::PARSE, Boundary::Format)
        .with_detail(format!("fault manifest: {detail}"))
}

fn take<'a>(fields: &mut Fields<'a>, key: &str) -> Result<&'a Json, Diag> {
    fields
        .take(key)
        .map_err(|error| field_error(format!("{error:?} field `{key}`")))
}

fn as_u64(value: &Json, key: &str) -> Result<u64, Diag> {
    json::as_u64(value).map_err(|detail| field_error(format!("{key}: {detail}")))
}

fn as_u32(value: &Json, key: &str) -> Result<u32, Diag> {
    let value = as_u64(value, key)?;
    u32::try_from(value).map_err(|_| field_error(format!("{key} out of u32 range: {value}")))
}

fn as_i64(value: &Json, key: &str) -> Result<i64, Diag> {
    json::as_i64(value).map_err(|detail| field_error(format!("{key}: {detail}")))
}

fn as_str<'a>(value: &'a Json, key: &str) -> Result<&'a str, Diag> {
    json::as_str(value).map_err(|detail| field_error(format!("{key}: {detail}")))
}

fn as_bool(value: &Json, key: &str) -> Result<bool, Diag> {
    json::as_bool(value).map_err(|detail| field_error(format!("{key}: {detail}")))
}

fn as_obj<'a>(value: &'a Json, key: &str) -> Result<&'a [(String, Json)], Diag> {
    json::as_obj(value).map_err(|detail| field_error(format!("{key}: {detail}")))
}

fn as_arr<'a>(value: &'a Json, key: &str) -> Result<&'a [Json], Diag> {
    json::as_arr(value).map_err(|detail| field_error(format!("{key}: {detail}")))
}

fn parse_manifest(obj: &[(String, Json)]) -> Result<FaultManifest, Diag> {
    let mut fields = Fields::new(obj);
    let schema = as_u64(take(&mut fields, "schema")?, "schema")?;
    if schema != FAULT_MANIFEST_SCHEMA as u64 {
        return Err(field_error(format!(
            "unsupported fault manifest schema {schema}; this build accepts {FAULT_MANIFEST_SCHEMA}"
        )));
    }
    let kind = as_str(take(&mut fields, "kind")?, "kind")?.to_string();
    if kind != "fault_dump" {
        return Err(field_error(format!("unexpected kind `{kind}`")));
    }
    let trace_sha256 = as_str(take(&mut fields, "trace_sha256")?, "trace_sha256")?.to_string();
    if trace_sha256.len() != 64 {
        return Err(field_error(format!(
            "trace_sha256 must be 64 hex digits, got {trace_sha256:?}"
        )));
    }
    let producer = parse_producer(take(&mut fields, "producer")?)?;
    let codec = as_str(take(&mut fields, "codec")?, "codec")?.to_string();
    let (fault_info, fault_tick, fault_frame) = parse_fault(take(&mut fields, "fault")?)?;
    let ring = parse_ring(take(&mut fields, "ring")?)?;
    let input = parse_input(take(&mut fields, "input")?)?;
    let clock = parse_clock(take(&mut fields, "clock")?)?;
    let start_frame = as_u64(take(&mut fields, "start_frame")?, "start_frame")?;
    let last_frame = as_u64(take(&mut fields, "last_frame")?, "last_frame")?;
    let orphan_events = as_u64(take(&mut fields, "orphan_events")?, "orphan_events")?;
    let vfs_records = as_u64(take(&mut fields, "vfs_records")?, "vfs_records")?;
    let vfs_dropped = as_u64(take(&mut fields, "vfs_dropped")?, "vfs_dropped")?;
    let boot_records_dropped = as_u64(
        take(&mut fields, "boot_records_dropped")?,
        "boot_records_dropped",
    )?;
    let timeline_truncated = as_bool(
        take(&mut fields, "timeline_truncated")?,
        "timeline_truncated",
    )?;
    let clock_truncated = as_bool(take(&mut fields, "clock_truncated")?, "clock_truncated")?;
    let evicted_ticks = as_u64(take(&mut fields, "evicted_ticks")?, "evicted_ticks")?;
    let ticks_value = take(&mut fields, "ticks")?;
    let mut ticks = Vec::new();
    for item in as_arr(ticks_value, "ticks")? {
        ticks.push(parse_tick(item)?);
    }
    fields
        .finish()
        .map_err(|error| field_error(format!("{error:?}")))?;
    Ok(FaultManifest {
        trace_sha256,
        producer,
        codec,
        fault: fault_info,
        fault_tick,
        fault_frame,
        ring,
        input,
        clock,
        start_frame,
        last_frame,
        orphan_events,
        vfs_records,
        vfs_dropped,
        boot_records_dropped,
        timeline_truncated,
        clock_truncated,
        evicted_ticks,
        ticks,
    })
}

fn parse_producer(value: &Json) -> Result<crate::record::ProducerInfo, Diag> {
    let obj = as_obj(value, "producer")?;
    let mut fields = Fields::new(obj);
    let kind = as_str(take(&mut fields, "kind")?, "producer.kind")?.to_string();
    let kind = crate::record::ProducerKind::parse(&kind)
        .ok_or_else(|| field_error(format!("unknown producer kind `{kind}`")))?;
    let name = as_str(take(&mut fields, "name")?, "producer.name")?.to_string();
    let version = as_str(take(&mut fields, "version")?, "producer.version")?.to_string();
    let sample_sha256 = match take(&mut fields, "sample_sha256")? {
        Json::Null => None,
        other => Some(as_str(other, "producer.sample_sha256")?.to_string()),
    };
    fields
        .finish()
        .map_err(|error| field_error(format!("{error:?}")))?;
    Ok(crate::record::ProducerInfo {
        kind,
        name,
        version,
        sample_sha256,
    })
}

fn parse_fault(value: &Json) -> Result<(FaultInfo, Option<u64>, u64), Diag> {
    let obj = as_obj(value, "fault")?;
    let mut fields = Fields::new(obj);
    let kind_text = as_str(take(&mut fields, "kind")?, "fault.kind")?.to_string();
    let kind = FaultKind::parse(&kind_text)
        .ok_or_else(|| field_error(format!("unknown fault kind `{kind_text}`")))?;
    let reason = as_str(take(&mut fields, "reason")?, "fault.reason")?.to_string();
    let detail = as_str(take(&mut fields, "detail")?, "fault.detail")?.to_string();
    let tick = match take(&mut fields, "tick")? {
        Json::Null => None,
        other => Some(as_u64(other, "fault.tick")?),
    };
    let frame = as_u64(take(&mut fields, "frame")?, "fault.frame")?;
    fields
        .finish()
        .map_err(|error| field_error(format!("{error:?}")))?;
    Ok((
        FaultInfo {
            kind,
            reason,
            detail,
        },
        tick,
        frame,
    ))
}

fn parse_ring(value: &Json) -> Result<RingConfig, Diag> {
    let obj = as_obj(value, "ring")?;
    let mut fields = Fields::new(obj);
    let config = RingConfig {
        ticks: as_u64(take(&mut fields, "ticks")?, "ring.ticks")? as usize,
        max_tick_bytes: as_u64(take(&mut fields, "max_tick_bytes")?, "ring.max_tick_bytes")?
            as usize,
        max_total_bytes: as_u64(
            take(&mut fields, "max_total_bytes")?,
            "ring.max_total_bytes",
        )? as usize,
        max_boot_records: as_u64(
            take(&mut fields, "max_boot_records")?,
            "ring.max_boot_records",
        )? as usize,
        max_diags_per_tick: as_u64(
            take(&mut fields, "max_diags_per_tick")?,
            "ring.max_diags_per_tick",
        )? as usize,
        max_inputs_per_tick: as_u64(
            take(&mut fields, "max_inputs_per_tick")?,
            "ring.max_inputs_per_tick",
        )? as usize,
        max_display_bytes: as_u64(
            take(&mut fields, "max_display_bytes")?,
            "ring.max_display_bytes",
        )? as usize,
        max_timeline_entries: as_u64(
            take(&mut fields, "max_timeline_entries")?,
            "ring.max_timeline_entries",
        )? as usize,
        max_clock_ticks: as_u64(
            take(&mut fields, "max_clock_ticks")?,
            "ring.max_clock_ticks",
        )? as usize,
    };
    fields
        .finish()
        .map_err(|error| field_error(format!("{error:?}")))?;
    config.validate()?;
    Ok(config)
}

fn parse_input(value: &Json) -> Result<FaultInput, Diag> {
    let obj = as_obj(value, "input")?;
    let mut fields = Fields::new(obj);
    let events = as_u64(take(&mut fields, "events")?, "input.events")?;
    let sha256 = as_str(take(&mut fields, "sha256")?, "input.sha256")?.to_string();
    let truncated = as_bool(take(&mut fields, "truncated")?, "input.truncated")?;
    let mut entries = Vec::new();
    for item in as_arr(take(&mut fields, "entries")?, "input.entries")? {
        entries.push(parse_input_entry(item)?);
    }
    fields
        .finish()
        .map_err(|error| field_error(format!("{error:?}")))?;
    Ok(FaultInput {
        events,
        sha256,
        entries,
        truncated,
    })
}

fn parse_input_entry(value: &Json) -> Result<InputEntry, Diag> {
    let obj = as_obj(value, "input entry")?;
    let mut fields = Fields::new(obj);
    let frame = as_u64(take(&mut fields, "frame")?, "input.frame")?;
    let cursor_values = as_arr(take(&mut fields, "cursor")?, "input.cursor")?;
    if cursor_values.len() != 2 {
        return Err(field_error(
            "input.cursor must have two integers".to_string(),
        ));
    }
    let cursor = (
        as_i64(&cursor_values[0], "input.cursor[0]")?,
        as_i64(&cursor_values[1], "input.cursor[1]")?,
    );
    let mut pressed = Vec::new();
    for item in as_arr(take(&mut fields, "pressed")?, "input.pressed")? {
        pressed.push(as_str(item, "input.pressed[]")?.to_string());
    }
    let mut pulses = Vec::new();
    for item in as_arr(take(&mut fields, "pulses")?, "input.pulses")? {
        pulses.push(as_str(item, "input.pulses[]")?.to_string());
    }
    fields
        .finish()
        .map_err(|error| field_error(format!("{error:?}")))?;
    Ok(InputEntry {
        frame,
        cursor,
        pressed,
        pulses,
    })
}

fn parse_clock(value: &Json) -> Result<ClockSummary, Diag> {
    let obj = as_obj(value, "clock")?;
    let mut fields = Fields::new(obj);
    let mode_text = as_str(take(&mut fields, "mode")?, "clock.mode")?.to_string();
    let mode = crate::record::ClockMode::parse(&mode_text)
        .ok_or_else(|| field_error(format!("unknown clock mode `{mode_text}`")))?;
    let mut now_ms = Vec::new();
    for item in as_arr(take(&mut fields, "now_ms")?, "clock.now_ms")? {
        match item {
            Json::Null => now_ms.push(None),
            other => now_ms.push(Some(as_u32(other, "clock.now_ms[]")?)),
        }
    }
    fields
        .finish()
        .map_err(|error| field_error(format!("{error:?}")))?;
    Ok(ClockSummary { mode, now_ms })
}

fn parse_tick(value: &Json) -> Result<TickSummary, Diag> {
    let obj = as_obj(value, "tick")?;
    let mut fields = Fields::new(obj);
    let index = as_u64(take(&mut fields, "index")?, "tick.index")?;
    let first_frame = as_u64(take(&mut fields, "first_frame")?, "tick.first_frame")?;
    let frames = as_u64(take(&mut fields, "frames")?, "tick.frames")?;
    let last_frame = as_u64(take(&mut fields, "last_frame")?, "tick.last_frame")?;
    let now_ms = match take(&mut fields, "now_ms")? {
        Json::Null => None,
        other => Some(as_u32(other, "tick.now_ms")?),
    };
    let command_count = as_u64(take(&mut fields, "command_count")?, "tick.command_count")?;
    let command_digest =
        as_str(take(&mut fields, "command_digest")?, "tick.command_digest")?.to_string();
    let event_digest = as_str(take(&mut fields, "event_digest")?, "tick.event_digest")?.to_string();
    let event_counts = parse_event_counts(take(&mut fields, "event_counts")?)?;
    let mut diags = Vec::new();
    for item in as_arr(take(&mut fields, "diags")?, "tick.diags")? {
        let entry_obj = as_obj(item, "tick.diags[]")?;
        let mut entry_fields = Fields::new(entry_obj);
        let frame = as_u64(take(&mut entry_fields, "frame")?, "diag.frame")?;
        let code = as_str(take(&mut entry_fields, "code")?, "diag.code")?.to_string();
        let detail = as_str(take(&mut entry_fields, "detail")?, "diag.detail")?.to_string();
        let unimplemented = as_bool(
            take(&mut entry_fields, "unimplemented")?,
            "diag.unimplemented",
        )?;
        entry_fields
            .finish()
            .map_err(|error| field_error(format!("{error:?}")))?;
        diags.push(DiagEntry {
            frame,
            code,
            detail,
            unimplemented,
        });
    }
    let diags_dropped = as_u64(take(&mut fields, "diags_dropped")?, "tick.diags_dropped")?;
    let mut inputs = Vec::new();
    for item in as_arr(take(&mut fields, "inputs")?, "tick.inputs")? {
        inputs.push(parse_input_entry(item)?);
    }
    let inputs_dropped = as_u64(take(&mut fields, "inputs_dropped")?, "tick.inputs_dropped")?;
    let mut frames_desc = Vec::new();
    for item in as_arr(
        take(&mut fields, "frame_descriptions")?,
        "tick.frame_descriptions",
    )? {
        let entry_obj = as_obj(item, "tick.frames[]")?;
        let mut entry_fields = Fields::new(entry_obj);
        let frame = as_u64(take(&mut entry_fields, "frame")?, "frame.frame")?;
        let display_sha256 = as_str(
            take(&mut entry_fields, "display_sha256")?,
            "frame.display_sha256",
        )?
        .to_string();
        let display_retained = as_bool(
            take(&mut entry_fields, "display_retained")?,
            "frame.display_retained",
        )?;
        let pass_count = as_u32(take(&mut entry_fields, "pass_count")?, "frame.pass_count")?;
        let op_count = as_u64(take(&mut entry_fields, "op_count")?, "frame.op_count")?;
        let executable_ops = as_u64(
            take(&mut entry_fields, "executable_ops")?,
            "frame.executable_ops",
        )?;
        let skipped_ops = as_u64(take(&mut entry_fields, "skipped_ops")?, "frame.skipped_ops")?;
        entry_fields
            .finish()
            .map_err(|error| field_error(format!("{error:?}")))?;
        frames_desc.push(FrameTickEntry {
            frame,
            display_sha256,
            display_retained,
            pass_count,
            op_count,
            executable_ops,
            skipped_ops,
        });
    }
    let displays_dropped = as_u64(
        take(&mut fields, "displays_dropped")?,
        "tick.displays_dropped",
    )?;
    let bytes = as_u64(take(&mut fields, "bytes")?, "tick.bytes")? as usize;
    let over_budget = as_bool(take(&mut fields, "over_budget")?, "tick.over_budget")?;
    let retained = as_bool(take(&mut fields, "retained")?, "tick.retained")?;
    fields
        .finish()
        .map_err(|error| field_error(format!("{error:?}")))?;
    Ok(TickSummary {
        index,
        first_frame,
        frames,
        last_frame,
        now_ms,
        command_count,
        command_digest,
        event_digest,
        event_counts,
        diags,
        diags_dropped,
        inputs,
        inputs_dropped,
        frames_desc,
        displays_dropped,
        bytes,
        over_budget,
        retained,
        entries: Vec::new(),
    })
}

// `event_counts` 的 JSON 是对象而不是数组;上面的解析按数组写会拒绝自己的输出。
// 这里改成对象解析:保持 `Fields` 的单次消费语义。
fn parse_event_counts(value: &Json) -> Result<BTreeMap<String, u64>, Diag> {
    let obj = as_obj(value, "tick.event_counts")?;
    let mut out = BTreeMap::new();
    for (key, item) in obj {
        out.insert(key.clone(), as_u64(item, "tick.event_counts[]")?);
    }
    Ok(out)
}

/// 供 `EventRecorder` 调用的 dump 入口(不加锁;调用方持有 ring 的锁)。
pub(crate) fn write_dump(
    ring: &mut FaultRing,
    path: &Path,
    fault: FaultInfo,
) -> Result<FaultDumpPaths, Diag> {
    // 正在记录的 tick 也收口:故障现场包含它。
    ring.finish_tick();
    let header = ring.dump_header(&fault)?;
    let log = ring.build_log(header.clone());
    let trace_text = log.to_jsonl();
    let trace_sha256 = sha256_hex(trace_text.as_bytes());
    let manifest = ring.manifest(fault, trace_sha256, &header);
    let paths = FaultDumpPaths::for_trace(path);
    manifest.write_bundle(&paths, &trace_text)?;
    Ok(paths)
}
