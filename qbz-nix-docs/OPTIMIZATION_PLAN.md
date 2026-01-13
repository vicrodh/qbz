# QBZ Performance Optimization Plan

## Problem Statement
QBZ consumes excessive resources when idle:
- **~934MB RAM** (583MB main process + 351MB WebKitWebProcess)
- **51 threads**
- **~1.5% CPU** without playing audio
- User was viewing ArtistDetailView (many images, long page)

## Root Cause Analysis

### Backend (Rust/Tauri)

| Issue | Location | Impact |
|-------|----------|--------|
| Playback polling loop | `src-tauri/src/lib.rs:170-206` | 250ms interval, runs ALWAYS |
| Audio thread polling | `src-tauri/src/player/mod.rs:498` | 100ms timeout, runs ALWAYS |
| Media server HTTP polling | `src-tauri/src/cast/media_server.rs:62` | 250ms timeout, runs ALWAYS |
| Audio cache in memory | `src-tauri/src/cache/mod.rs` | 500MB default limit |

### Frontend (Svelte/WebKit)

| Issue | Location | Impact |
|-------|----------|--------|
| **Lyrics 50ms interval** | `src/lib/stores/lyricsStore.ts:389` | 20 updates/sec even when hidden |
| IntersectionObserver (4 thresholds) | `src/lib/components/views/ArtistDetailView.svelte:300` | Fires on every scroll |
| No lazy loading for images | Multiple components | All images load immediately |
| Transform scale on hover (JS) | `src/lib/components/AlbumCard.svelte:37` | Reflows on every hover |
| 7 store subscriptions cascade | `src/routes/+page.svelte:1238-1336` | All fire on playback updates |
| No list virtualization | QueuePanel, Favorites | Large DOM trees |
| backdrop-filter blur | `src/lib/components/glass/GlassSurface.svelte:122` | GPU intensive |

---

## Phase 1: Critical Fixes (Idle CPU)

### 1.1 Fix Lyrics Interval Leak
**File:** `src/lib/stores/lyricsStore.ts`

**Problem:** 50ms setInterval runs continuously even when lyrics sidebar is hidden.

**Fix:**
```typescript
// Ensure interval is ONLY running when lyrics are visible AND playing
export function startActiveLineUpdates(): void {
  if (updateInterval !== null) return;
  // Add visibility check here
  updateInterval = window.setInterval(() => {
    if (parsedLyrics.isSynced) {
      updateActiveLine();
    }
  }, 50);
}

// Add explicit cleanup
export function stopActiveLineUpdates(): void {
  if (updateInterval !== null) {
    window.clearInterval(updateInterval);
    updateInterval = null;
  }
}
```

**Also fix in +page.svelte:** Ensure the $effect properly cleans up on all state changes.

### 1.2 Conditional Playback Polling
**File:** `src-tauri/src/lib.rs:170-206`

**Problem:** 250ms polling loop runs forever, even when paused.

**Fix:** Only poll when playing:
```rust
loop {
    thread::sleep(Duration::from_millis(250));

    // Skip polling if not playing
    if !player_handle.is_playing() {
        thread::sleep(Duration::from_millis(1000)); // Slower poll when paused
        continue;
    }

    // ... rest of polling logic
}
```

### 1.3 Reduce Audio Thread Timeout When Idle
**File:** `src-tauri/src/player/mod.rs:498`

**Problem:** 100ms recv_timeout even when not playing.

**Fix:** Use longer timeout when idle:
```rust
let timeout = if is_playing.load(Ordering::Relaxed) {
    Duration::from_millis(100)
} else {
    Duration::from_millis(500)  // Slower when paused
};

match rx.recv_timeout(timeout) { ... }
```

---

## Phase 2: Image Optimization

### 2.1 Add Lazy Loading to All Images
**Files:**
- `src/lib/components/AlbumCard.svelte`
- `src/lib/components/views/ArtistDetailView.svelte`
- `src/lib/components/TrackRow.svelte`

**Fix:** Add `loading="lazy"` to all `<img>` tags:
```svelte
<img
  src={artworkUrl}
  alt={title}
  loading="lazy"
  decoding="async"
/>
```

### 2.2 Use Smaller Image Sizes from Qobuz API
**Current:** Using full-size images (e.g., 600x600)
**Fix:** Request smaller sizes for thumbnails:
- Cards: `?w=300`
- Sidebar: `?w=100`
- Detail headers: `?w=600`

### 2.3 Optimize IntersectionObserver in ArtistDetailView
**File:** `src/lib/components/views/ArtistDetailView.svelte:300-343`

**Fix:** Reduce thresholds and debounce:
```typescript
jumpObserver = new IntersectionObserver(
  (entries) => {
    // Debounce callback
    requestAnimationFrame(() => {
      const visible = entries.find(entry => entry.isIntersecting);
      if (visible) {
        const targetId = sectionByElement.get(visible.target as HTMLDivElement);
        if (targetId) activeJumpSection = targetId;
      }
    });
  },
  {
    root: artistDetailEl,
    rootMargin: '-20% 0px -60% 0px',
    threshold: [0.5]  // Single threshold instead of 4
  }
);
```

---

## Phase 3: CSS Performance

### 3.1 Use CSS :hover Instead of JS State
**File:** `src/lib/components/AlbumCard.svelte`

**Current (Bad):**
```svelte
<div style="transform: scale({isHovered ? 1.02 : 1})">
```

**Fix (Good):**
```svelte
<div class="artwork-container">

<style>
  .artwork-container {
    transition: transform 150ms ease-out;
  }
  .artwork-container:hover {
    transform: scale(1.02);
  }
</style>
```

### 3.2 Conditional Glass Effects
**File:** `src/lib/components/glass/GlassSurface.svelte`

**Fix:** Add prop to disable blur when not needed:
```svelte
<script>
  interface Props {
    enableBlur?: boolean;
  }
  let { enableBlur = true }: Props = $props();
</script>

<style>
  .glass-backdrop.blur-enabled {
    backdrop-filter: blur(var(--glass-blur, 16px)) saturate(140%);
  }
</style>
```

---

## Phase 4: Memory Optimization

### 4.1 Reduce Default Audio Cache Size
**File:** `src-tauri/src/cache/mod.rs`

**Current:** 500MB default
**Fix:** Reduce to 200MB or make configurable:
```rust
const DEFAULT_CACHE_SIZE: usize = 200 * 1024 * 1024; // 200MB
```

### 4.2 Add Cache Size Setting
**New setting in Settings UI:**
- Audio cache size: 100MB / 200MB / 500MB / 1GB
- Store in settings, pass to AudioCache on init

### 4.3 Lazy Initialize Cast Services
**File:** `src-tauri/src/lib.rs`

**Current:** Media server starts immediately on app launch.
**Fix:** Only start when user opens cast panel:
```rust
// Don't start in build()
// Start on first cast_start_discovery() call instead
```

---

## Phase 5: List Virtualization (Future)

### 5.1 Virtual Scrolling for QueuePanel
**File:** `src/lib/components/QueuePanel.svelte`

**Implementation:** Use svelte-virtual-list or custom implementation:
- Only render visible items + buffer
- Recycle DOM nodes on scroll
- Target: Handle 1000+ tracks smoothly

### 5.2 Virtual Scrolling for Favorites
Similar implementation for favorites list.

---

## Implementation Order

| Priority | Task | Impact | Effort |
|----------|------|--------|--------|
| P0 | Fix lyrics interval leak | HIGH | LOW |
| P0 | Conditional playback polling | HIGH | LOW |
| P1 | Lazy load images | HIGH | MEDIUM |
| P1 | Reduce IntersectionObserver thresholds | MEDIUM | LOW |
| P1 | CSS :hover instead of JS state | MEDIUM | LOW |
| P2 | Reduce audio thread timeout when idle | MEDIUM | LOW |
| P2 | Reduce default cache size | MEDIUM | LOW |
| P2 | Smaller image sizes from API | MEDIUM | MEDIUM |
| P3 | Lazy init cast services | LOW | MEDIUM |
| P3 | Conditional glass blur | LOW | LOW |
| P4 | List virtualization | MEDIUM | HIGH |

---

## Verification

### Before/After Metrics
1. Open btop or htop
2. Launch QBZ
3. Navigate to ArtistDetailView (same artist with many albums)
4. Pause playback
5. Wait 30 seconds
6. Record: CPU%, Memory, Thread count

### Expected Results After Optimization
- **CPU (idle):** < 0.5% (down from 1.5%)
- **RAM:** < 600MB total (down from ~934MB)
- **Threads:** < 30 (down from 51)

### Test Cases
1. [ ] App idle on home view - CPU should be ~0%
2. [ ] App idle on ArtistDetailView - CPU should be < 0.5%
3. [ ] Playback paused - CPU should drop to idle levels
4. [ ] Scroll through long artist page - no stuttering
5. [ ] Open/close lyrics sidebar - no CPU spike after close
6. [ ] Memory doesn't grow over time with normal use

---

## Files to Modify

| File | Changes |
|------|---------|
| `src/lib/stores/lyricsStore.ts` | Fix interval cleanup |
| `src-tauri/src/lib.rs` | Conditional polling |
| `src-tauri/src/player/mod.rs` | Dynamic recv_timeout |
| `src/lib/components/AlbumCard.svelte` | CSS hover, lazy img |
| `src/lib/components/views/ArtistDetailView.svelte` | Reduce thresholds, lazy img |
| `src/lib/components/TrackRow.svelte` | Lazy img |
| `src-tauri/src/cache/mod.rs` | Reduce default size |
| `src/lib/components/glass/GlassSurface.svelte` | Conditional blur |

---

## Notes

- WebKitWebProcess (351MB) is largely unavoidable - it's the WebView rendering engine
- Some memory is WebKit's internal caches and can't be controlled
- Focus on CPU optimization first as it's more noticeable to users
- Consider adding a "Low Resource Mode" toggle in settings for potato PCs
