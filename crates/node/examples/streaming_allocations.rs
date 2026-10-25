//! Component benchmark, not a browser/HTTP or physical disk latency claim.
//! Run in Release against a legally generated non-fragmented MP4/Opus fixture.
use http::Request;
use http_body_util::BodyExt;
use media_node::{OpenedFile, mp4_audio::NativeAudio};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

struct MeasuredAllocator;
static EVENTS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
fn allocated(size: usize) {
    EVENTS.fetch_add(1, Ordering::Relaxed);
    BYTES.fetch_add(size, Ordering::Relaxed);
    PEAK.fetch_max(
        LIVE.fetch_add(size, Ordering::Relaxed) + size,
        Ordering::Relaxed,
    );
}
// SAFETY: every operation delegates to System with the original pointer/layout;
// counters never allocate and do not change allocator semantics.
unsafe impl GlobalAlloc for MeasuredAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            allocated(layout.size());
        }
        ptr
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            allocated(layout.size());
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, ptr: *mut u8, old: Layout, new_size: usize) -> *mut u8 {
        let next = unsafe { System.realloc(ptr, old, new_size) };
        if !next.is_null() {
            LIVE.fetch_sub(old.size(), Ordering::Relaxed);
            allocated(new_size);
        }
        next
    }
}
#[global_allocator]
static ALLOCATOR: MeasuredAllocator = MeasuredAllocator;

#[tokio::main]
async fn main() -> io::Result<()> {
    let fixture = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/generated/dual-opus/dual-opus-long.mp4")
        });
    let fixture = fixture.canonicalize()?;
    let root = fixture
        .parent()
        .ok_or_else(|| io::Error::other("fixture root missing"))?
        .to_owned();
    let relative = PathBuf::from(
        fixture
            .file_name()
            .ok_or_else(|| io::Error::other("fixture name missing"))?,
    );
    let source = OpenedFile::open_in_root(&root, &relative).await?;
    let tracks = Arc::new(media_node::mp4_audio::supported_tracks(
        source.try_clone_std()?,
    )?);
    if tracks.is_empty() {
        return Err(io::Error::other("no supported native Opus track"));
    }
    for sessions in [4, 8, 16] {
        let native = Arc::new(NativeAudio::default());
        for (phase, rounds) in [("cold-index", 1), ("warm-index", 20)] {
            let start_events = EVENTS.load(Ordering::Relaxed);
            let start_bytes = BYTES.load(Ordering::Relaxed);
            let start_live = LIVE.load(Ordering::Relaxed);
            PEAK.store(start_live, Ordering::Relaxed);
            let started = Instant::now();
            let mut samples = Vec::new();
            for _ in 0..rounds {
                let mut tasks = tokio::task::JoinSet::new();
                for index in 0..sessions {
                    let (root, relative, native, tracks) = (
                        root.clone(),
                        relative.clone(),
                        native.clone(),
                        tracks.clone(),
                    );
                    tasks.spawn(async move {
                        let started = Instant::now();
                        let opened = OpenedFile::open_in_root(&root, &relative).await?;
                        let response = native
                            .serve(
                                opened,
                                tracks[index % tracks.len()],
                                0,
                                &Request::builder()
                                    .header("range", "bytes=0-65535")
                                    .body(())
                                    .map_err(io::Error::other)?,
                            )
                            .await?;
                        if response.status() != 206 {
                            return Err(io::Error::other("unexpected native Range status"));
                        }
                        let headers_us = started.elapsed().as_micros() as u64;
                        let mut body = response.into_body();
                        let mut bytes = 0;
                        while let Some(frame) = body.frame().await {
                            if let Ok(data) = frame?.into_data() {
                                bytes += data.len();
                            }
                        }
                        Ok::<_, io::Error>((
                            headers_us,
                            started.elapsed().as_micros() as u64,
                            bytes,
                        ))
                    });
                }
                while let Some(result) = tasks.join_next().await {
                    samples.push(result.map_err(io::Error::other)??);
                }
            }
            let elapsed_us = started.elapsed().as_micros();
            let events = EVENTS.load(Ordering::Relaxed) - start_events;
            let allocated_bytes = BYTES.load(Ordering::Relaxed) - start_bytes;
            let peak_additional_bytes = PEAK.load(Ordering::Relaxed).saturating_sub(start_live);
            println!(
                "{}",
                serde_json::json!({
                    "scope":"Instrumented System allocator: native Opus + Tokio tasks and file I/O; no HTTP/browser; OS cache not purged; capability scan precedes measurement",
                    "sessions":sessions,"phase":phase,"requests":samples.len(),"elapsed_us":elapsed_us,
                    "allocation_events":events,"allocated_bytes":allocated_bytes,"peak_additional_live_bytes":peak_additional_bytes,
                    "samples_headers_us_total_us_bytes":samples
                })
            );
        }
    }
    Ok(())
}
