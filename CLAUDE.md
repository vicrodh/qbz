# Claude Code Instructions for QBZ-NIX

## Critical CSS Rules

### NEVER add `display: flex` to `.main-content` in `+page.svelte`

**This has broken the app multiple times.**

The `.main-content` container in `src/routes/+page.svelte` must NOT have `display: flex` or `flex-direction: column` added to it.

**Why it breaks:**
- The child view components (HomeView, SearchView, FavoritesView, etc.) don't have `flex-grow` or explicit heights
- When the parent becomes a flex container with `flex-direction: column`, children collapse to zero height
- The components still mount (visible in console logs like "SearchView mounted!") but are invisible
- No JavaScript errors appear - it's purely a CSS layout issue

**Correct CSS for `.main-content`:**
```css
.main-content {
  flex: 1;
  min-width: 0;
  height: calc(100vh - 96px);  /* 96px = NowPlayingBar height */
  overflow-y: auto;
  padding: 24px 32px;
}
```

**Symptoms when broken:**
- Sidebar renders correctly
- NowPlayingBar renders correctly
- Content area is completely empty/black
- Console shows components mounting without errors
- Navigation logs show view changes happening correctly

---

## Project Structure

- **Frontend:** SvelteKit with Svelte 5 runes (`$state`, `$derived`, `$effect`, `$props`)
- **Backend:** Tauri 2.0 with Rust
- **Styling:** Scoped CSS in Svelte components with CSS variables

## Svelte 5 Notes

- Use `$state()` for reactive variables
- Use `$derived()` for computed values
- Use `$props()` for component props
- Use `$effect()` for side effects (replaces `$:` reactive statements)
