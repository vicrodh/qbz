<script lang="ts">
  import { tick } from 'svelte';
  import { t } from 'svelte-i18n';
  import { invoke } from '@tauri-apps/api/core';
  import { X, ChevronLeft, ChevronRight, ZoomIn, ZoomOut, Maximize, RotateCw } from 'lucide-svelte';
  import * as pdfjsLib from 'pdfjs-dist';

  // Serve worker from static/ (real HTTP URL, not Vite blob) so it
  // can resolve sibling WASM files (openjpeg.wasm, jbig2.wasm, etc.)
  pdfjsLib.GlobalWorkerOptions.workerSrc = '/pdfjs/pdf.worker.min.mjs';

  interface Props {
    isOpen: boolean;
    onClose: () => void;
    url: string;
    title?: string;
  }

  let {
    isOpen,
    onClose,
    url,
    title = ''
  }: Props = $props();

  let canvas: HTMLCanvasElement | null = $state(null);
  let pdfDoc: any = null;
  let currentPage = $state(1);
  let totalPages = $state(0);
  let zoom = $state(1);
  let rotation = $state(0);
  let isLoading = $state(true);
  let error = $state('');
  let currentRenderTask: any = null;
  let containerEl: HTMLDivElement | null = $state(null);

  const MIN_ZOOM = 0.5;
  const MAX_ZOOM = 5;
  const ZOOM_STEP = 0.25;

  async function loadPdf() {
    if (!url) return;

    isLoading = true;
    error = '';

    try {
      // Fetch PDF bytes through Tauri backend to bypass CORS
      const bytes = await invoke<number[]>('v2_fetch_url_bytes', { url });
      const data = new Uint8Array(bytes);

      const loadingTask = pdfjsLib.getDocument({
        data,
        disableRange: true,
        disableAutoFetch: true,
        wasmUrl: '/pdfjs/',
      } as any);

      pdfDoc = await loadingTask.promise;
      totalPages = pdfDoc.numPages;
      currentPage = 1;
      // Set loading false FIRST so canvas mounts in the DOM
      isLoading = false;
      await tick(); // Wait for Svelte to update DOM

      // Auto-fit: calculate zoom so page fills container width
      const firstPage = await pdfDoc.getPage(1);
      const baseVp = firstPage.getViewport({ scale: 1, rotation });
      const availableWidth = (containerEl?.clientWidth ?? window.innerWidth) - 48;
      zoom = Math.min(availableWidth / baseVp.width, MAX_ZOOM);

      await renderPage();
    } catch (err: any) {
      console.error('[BookletViewer] Failed to load PDF:', err);
      error = err?.message || String(err) || 'Failed to load booklet';
      isLoading = false;
    }
  }

  async function renderPage() {
    if (!pdfDoc || !canvas) return;

    if (currentRenderTask) {
      currentRenderTask.cancel();
      currentRenderTask = null;
    }

    try {
      const page = await pdfDoc.getPage(currentPage);
      const dpr = window.devicePixelRatio || 1;
      // Render at 3x for near-print quality (216 DPI at 1x screens)
      const qualityScale = 3;
      const scale = zoom * dpr * qualityScale;
      const viewport = page.getViewport({ scale, rotation });

      canvas.width = viewport.width;
      canvas.height = viewport.height;
      // CSS size = canvas pixels / (dpr * qualityScale)
      canvas.style.width = `${viewport.width / (dpr * qualityScale)}px`;
      canvas.style.height = `${viewport.height / (dpr * qualityScale)}px`;

      const context = canvas.getContext('2d');
      if (!context) return;

      context.fillStyle = 'white';
      context.fillRect(0, 0, canvas.width, canvas.height);

      currentRenderTask = page.render({
        canvasContext: context,
        viewport,
      });
      await currentRenderTask.promise;
      currentRenderTask = null;
    } catch (err: any) {
      if (err?.name !== 'RenderingCancelledException') {
        console.error('[BookletViewer] Render error:', err);
      }
    }
  }

  function prevPage() {
    if (currentPage > 1) {
      currentPage--;
      renderPage();
    }
  }

  function nextPage() {
    if (currentPage < totalPages) {
      currentPage++;
      renderPage();
    }
  }

  function zoomIn() {
    if (zoom < MAX_ZOOM) {
      zoom = Math.min(zoom + ZOOM_STEP, MAX_ZOOM);
      renderPage();
    }
  }

  function zoomOut() {
    if (zoom > MIN_ZOOM) {
      zoom = Math.max(zoom - ZOOM_STEP, MIN_ZOOM);
      renderPage();
    }
  }

  async function fitToWidth() {
    if (!pdfDoc) return;
    const page = await pdfDoc.getPage(currentPage);
    const baseVp = page.getViewport({ scale: 1, rotation });
    const availableWidth = (containerEl?.clientWidth ?? window.innerWidth) - 48;
    zoom = Math.min(availableWidth / baseVp.width, MAX_ZOOM);
    renderPage();
  }

  function rotate() {
    rotation = (rotation + 90) % 360;
    renderPage();
  }

  function handleKeydown(e: KeyboardEvent) {
    if (!isOpen) return;

    switch (e.key) {
      case 'Escape':
        onClose();
        break;
      case 'ArrowLeft':
        prevPage();
        break;
      case 'ArrowRight':
        nextPage();
        break;
      case '+':
      case '=':
        zoomIn();
        break;
      case '-':
        zoomOut();
        break;
      case 'r':
        rotate();
        break;
    }
  }

  function handleBackdropClick(e: MouseEvent) {
    if (e.target === e.currentTarget) {
      onClose();
    }
  }

  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return {
      destroy() {
        if (pdfDoc) {
          pdfDoc.destroy();
          pdfDoc = null;
        }
        node.remove();
      }
    };
  }

  $effect(() => {
    if (isOpen && url) {
      loadPdf();
    }
  });

  $effect(() => {
    if (!isOpen && pdfDoc) {
      pdfDoc.destroy();
      pdfDoc = null;
      currentPage = 1;
      totalPages = 0;
      zoom = 1;
      rotation = 0;
    }
  });
</script>

<svelte:window onkeydown={handleKeydown} />

{#if isOpen}
  <div class="booklet-overlay" use:portal onclick={handleBackdropClick}>
    <!-- Toolbar -->
    <div class="booklet-toolbar">
      <div class="toolbar-left">
        <span class="booklet-title">{title || $t('album.booklet')}</span>
      </div>
      <div class="toolbar-center">
        <button class="toolbar-btn" onclick={prevPage} disabled={currentPage <= 1}>
          <ChevronLeft size={18} />
        </button>
        <span class="page-indicator">
          {currentPage} / {totalPages}
        </span>
        <button class="toolbar-btn" onclick={nextPage} disabled={currentPage >= totalPages}>
          <ChevronRight size={18} />
        </button>
        <div class="toolbar-divider"></div>
        <button class="toolbar-btn" onclick={zoomOut} disabled={zoom <= MIN_ZOOM}>
          <ZoomOut size={16} />
        </button>
        <span class="zoom-indicator">{Math.round(zoom * 100)}%</span>
        <button class="toolbar-btn" onclick={zoomIn} disabled={zoom >= MAX_ZOOM}>
          <ZoomIn size={16} />
        </button>
        <button class="toolbar-btn" onclick={fitToWidth} title={$t('album.bookletFitWidth')}>
          <Maximize size={16} />
        </button>
        <div class="toolbar-divider"></div>
        <button class="toolbar-btn" onclick={rotate} title={$t('album.bookletRotate')}>
          <RotateCw size={16} />
        </button>
      </div>
      <div class="toolbar-right">
        <button class="toolbar-btn close-btn" onclick={onClose}>
          <X size={18} />
        </button>
      </div>
    </div>

    <!-- Content -->
    <div class="booklet-content" bind:this={containerEl}>
      {#if isLoading}
        <div class="booklet-loading">
          <div class="spinner"></div>
          <span>{$t('album.bookletLoading')}</span>
        </div>
      {:else if error}
        <div class="booklet-error">
          <span>{$t('album.bookletError')}</span>
          <span class="error-detail">{error}</span>
        </div>
      {:else}
        <div class="canvas-wrapper">
          <canvas bind:this={canvas}></canvas>
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .booklet-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.85);
    display: flex;
    flex-direction: column;
    z-index: 200000;
    animation: booklet-fade-in 150ms ease;
  }

  @keyframes booklet-fade-in {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  .booklet-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 16px;
    background: rgba(0, 0, 0, 0.7);
    backdrop-filter: blur(12px);
    border-bottom: 1px solid rgba(255, 255, 255, 0.1);
    flex-shrink: 0;
    -webkit-app-region: no-drag;
  }

  .toolbar-left,
  .toolbar-right {
    flex: 1;
    display: flex;
    align-items: center;
  }

  .toolbar-right {
    justify-content: flex-end;
  }

  .toolbar-center {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .booklet-title {
    color: rgba(255, 255, 255, 0.8);
    font-size: 13px;
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 200px;
  }

  .toolbar-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    background: transparent;
    border: none;
    border-radius: 6px;
    color: rgba(255, 255, 255, 0.8);
    cursor: pointer;
    transition: background 120ms ease, color 120ms ease;
  }

  .toolbar-btn:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.1);
    color: white;
  }

  .toolbar-btn:disabled {
    opacity: 0.3;
    cursor: default;
  }

  .close-btn:hover {
    background: rgba(255, 60, 60, 0.3);
    color: #ff6b6b;
  }

  .page-indicator,
  .zoom-indicator {
    color: rgba(255, 255, 255, 0.7);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    min-width: 48px;
    text-align: center;
  }

  .toolbar-divider {
    width: 1px;
    height: 20px;
    background: rgba(255, 255, 255, 0.15);
    margin: 0 4px;
  }

  .booklet-content {
    flex: 1;
    overflow: auto;
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding: 24px;
  }

  .canvas-wrapper {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 100%;
  }

  .canvas-wrapper canvas {
    border-radius: 4px;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
  }

  .booklet-loading,
  .booklet-error {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    color: rgba(255, 255, 255, 0.6);
    font-size: 14px;
    height: 100%;
    align-self: center;
  }

  .error-detail {
    font-size: 12px;
    color: rgba(255, 255, 255, 0.35);
    max-width: 400px;
    text-align: center;
  }

  .spinner {
    width: 32px;
    height: 32px;
    border: 3px solid rgba(255, 255, 255, 0.15);
    border-top-color: rgba(255, 255, 255, 0.6);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }
</style>
