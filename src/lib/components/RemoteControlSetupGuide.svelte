<script lang="ts">
  import { X, CheckCircle, Smartphone, Shield, Key, QrCode, Wifi } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import WizardStepper, { type Step } from './wizard/WizardStepper.svelte';

  interface Props {
    isOpen: boolean;
    onClose: () => void;
  }

  let { isOpen, onClose }: Props = $props();

  type WizardStep = 'welcome' | 'enable' | 'https' | 'certificate' | 'pair' | 'done';

  const STEPS: WizardStep[] = ['welcome', 'enable', 'https', 'certificate', 'pair', 'done'];

  // Wizard state
  let currentStep = $state<WizardStep>('welcome');
  let completedSteps = $state(new Set<WizardStep>());
  let selectedPlatform = $state<'ios' | 'macos' | 'windows' | 'android'>('ios');

  // Reset state when modal opens
  $effect(() => {
    if (isOpen) {
      currentStep = 'welcome';
      completedSteps = new Set();
      selectedPlatform = 'ios';
    }
  });

  // Derived values
  const currentIndex = $derived(STEPS.indexOf(currentStep));

  // Store labelKey instead of calling $t() in $derived - resolve in WizardStepper template
  const steps = $derived<(Step & { labelKey: string })[]>(STEPS.map((step, index) => ({
    id: step,
    label: step, // fallback
    labelKey: `remoteControlWizard.steps.${step}`,
    status: index === currentIndex ? 'active' : completedSteps.has(step) ? 'complete' : 'upcoming'
  })));

  // Navigation
  function goToStep(stepId: string) {
    const step = stepId as WizardStep;
    if (completedSteps.has(step)) {
      currentStep = step;
    }
  }

  function next() {
    completedSteps.add(currentStep);
    completedSteps = new Set(completedSteps);

    const nextIndex = currentIndex + 1;
    if (nextIndex < STEPS.length) {
      currentStep = STEPS[nextIndex];
    }
  }

  function back() {
    const prevIndex = currentIndex - 1;
    if (prevIndex >= 0) {
      currentStep = STEPS[prevIndex];
    }
  }

  function handleClose() {
    onClose();
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      handleClose();
    }
  }

  function handleBackdropClick(e: MouseEvent) {
    if (e.target === e.currentTarget) {
      handleClose();
    }
  }
</script>

<svelte:document onkeydown={handleKeydown} />

{#if isOpen}
  <div class="wizard-backdrop" onclick={handleBackdropClick} role="presentation">
    <div
      class="wizard-modal"
      role="dialog"
      aria-modal="true"
      onclick={(e) => e.stopPropagation()}
    >
      <!-- Header -->
      <header class="wizard-header">
        <div class="header-content">
          {#if currentStep === 'welcome'}
            <h2>{$t('remoteControlWizard.welcome.title')}</h2>
            <p>{$t('remoteControlWizard.welcome.subtitle')}</p>
          {:else if currentStep === 'enable'}
            <h2>{$t('remoteControlWizard.enable.title')}</h2>
            <p>{$t('remoteControlWizard.enable.subtitle')}</p>
          {:else if currentStep === 'https'}
            <h2>{$t('remoteControlWizard.https.title')}</h2>
            <p>{$t('remoteControlWizard.https.subtitle')}</p>
          {:else if currentStep === 'certificate'}
            <h2>{$t('remoteControlWizard.certificate.title')}</h2>
            <p>{$t('remoteControlWizard.certificate.subtitle')}</p>
          {:else if currentStep === 'pair'}
            <h2>{$t('remoteControlWizard.pair.title')}</h2>
            <p>{$t('remoteControlWizard.pair.subtitle')}</p>
          {:else if currentStep === 'done'}
            <h2>{$t('remoteControlWizard.done.title')}</h2>
            <p>{$t('remoteControlWizard.done.subtitle')}</p>
          {/if}
        </div>
        <button class="close-btn" onclick={handleClose}>
          <X size={20} />
        </button>
      </header>

      <!-- Body -->
      <div class="wizard-body">
        {#if currentStep !== 'done'}
          <WizardStepper {steps} onStepClick={goToStep} />
        {/if}

        <div class="wizard-content">
          {#if currentStep === 'welcome'}
            <div class="step-content welcome-content">
              <div class="welcome-icon">
                <Smartphone size={48} />
              </div>

              <p class="body-text intro-text">{$t('remoteControlWizard.welcome.intro')}</p>

              <div class="feature-list">
                <div class="feature-item">
                  <CheckCircle size={16} class="feature-check" />
                  <span>{$t('remoteControlWizard.welcome.feature1')}</span>
                </div>
                <div class="feature-item">
                  <CheckCircle size={16} class="feature-check" />
                  <span>{$t('remoteControlWizard.welcome.feature2')}</span>
                </div>
                <div class="feature-item">
                  <CheckCircle size={16} class="feature-check" />
                  <span>{$t('remoteControlWizard.welcome.feature3')}</span>
                </div>
                <div class="feature-item">
                  <CheckCircle size={16} class="feature-check" />
                  <span>{$t('remoteControlWizard.welcome.feature4')}</span>
                </div>
              </div>

              <div class="screenshot-placeholder">
                <span class="placeholder-text">{$t('remoteControlWizard.welcome.screenshot')}</span>
              </div>
            </div>

          {:else if currentStep === 'enable'}
            <div class="step-content">
              <div class="step-icon">
                <Wifi size={32} />
              </div>

              <ol class="step-list">
                <li>{$t('remoteControlWizard.enable.step1')}</li>
                <li>{$t('remoteControlWizard.enable.step2')}</li>
                <li>{$t('remoteControlWizard.enable.step3')}</li>
              </ol>

              <p class="note-text">{$t('remoteControlWizard.enable.portNote')}</p>

              <div class="screenshot-placeholder">
                <span class="placeholder-text">{$t('remoteControlWizard.enable.screenshot')}</span>
              </div>
            </div>

          {:else if currentStep === 'https'}
            <div class="step-content">
              <div class="step-icon">
                <Shield size={32} />
              </div>

              <p class="body-text">{$t('remoteControlWizard.https.intro')}</p>

              <ol class="step-list">
                <li>{$t('remoteControlWizard.https.step1')}</li>
                <li>{$t('remoteControlWizard.https.step2')}</li>
                <li>{$t('remoteControlWizard.https.step3')}</li>
              </ol>

              <div class="why-section">
                <h4>{$t('remoteControlWizard.https.whyRequired')}</h4>
                <ul class="why-list">
                  <li>{$t('remoteControlWizard.https.whyReason1')}</li>
                  <li>{$t('remoteControlWizard.https.whyReason2')}</li>
                  <li>{$t('remoteControlWizard.https.whyReason3')}</li>
                </ul>
              </div>

              <div class="screenshot-placeholder">
                <span class="placeholder-text">{$t('remoteControlWizard.https.screenshot')}</span>
              </div>
            </div>

          {:else if currentStep === 'certificate'}
            <div class="step-content certificate-content">
              <div class="step-icon">
                <Key size={32} />
              </div>

              <p class="body-text">{$t('remoteControlWizard.certificate.intro')}</p>

              <ol class="step-list main-steps">
                <li>{$t('remoteControlWizard.certificate.step1')}</li>
                <li>{$t('remoteControlWizard.certificate.step2')}</li>
                <li>{$t('remoteControlWizard.certificate.step3')}</li>
                <li>{$t('remoteControlWizard.certificate.step4')}</li>
              </ol>

              <div class="platform-tabs">
                <button
                  class="platform-tab"
                  class:active={selectedPlatform === 'ios'}
                  onclick={() => selectedPlatform = 'ios'}
                >
                  iOS
                </button>
                <button
                  class="platform-tab"
                  class:active={selectedPlatform === 'macos'}
                  onclick={() => selectedPlatform = 'macos'}
                >
                  macOS
                </button>
                <button
                  class="platform-tab"
                  class:active={selectedPlatform === 'windows'}
                  onclick={() => selectedPlatform = 'windows'}
                >
                  Windows
                </button>
                <button
                  class="platform-tab"
                  class:active={selectedPlatform === 'android'}
                  onclick={() => selectedPlatform = 'android'}
                >
                  Android
                </button>
              </div>

              <div class="platform-instructions">
                {#if selectedPlatform === 'ios'}
                  <h4>{$t('remoteControlWizard.certificate.iosTitle')}</h4>
                  <ol class="platform-steps">
                    <li>{$t('remoteControlWizard.certificate.iosStep1')}</li>
                    <li>{$t('remoteControlWizard.certificate.iosStep2')}</li>
                    <li>{$t('remoteControlWizard.certificate.iosStep3')}</li>
                    <li>{$t('remoteControlWizard.certificate.iosStep4')}</li>
                    <li>{$t('remoteControlWizard.certificate.iosStep5')}</li>
                  </ol>
                {:else if selectedPlatform === 'macos'}
                  <h4>{$t('remoteControlWizard.certificate.macosTitle')}</h4>
                  <ol class="platform-steps">
                    <li>{$t('remoteControlWizard.certificate.macosStep1')}</li>
                    <li>{$t('remoteControlWizard.certificate.macosStep2')}</li>
                    <li>{$t('remoteControlWizard.certificate.macosStep3')}</li>
                    <li>{$t('remoteControlWizard.certificate.macosStep4')}</li>
                    <li>{$t('remoteControlWizard.certificate.macosStep5')}</li>
                  </ol>
                {:else if selectedPlatform === 'windows'}
                  <h4>{$t('remoteControlWizard.certificate.windowsTitle')}</h4>
                  <ol class="platform-steps">
                    <li>{$t('remoteControlWizard.certificate.windowsStep1')}</li>
                    <li>{$t('remoteControlWizard.certificate.windowsStep2')}</li>
                    <li>{$t('remoteControlWizard.certificate.windowsStep3')}</li>
                    <li>{$t('remoteControlWizard.certificate.windowsStep4')}</li>
                    <li>{$t('remoteControlWizard.certificate.windowsStep5')}</li>
                    <li>{$t('remoteControlWizard.certificate.windowsStep6')}</li>
                  </ol>
                {:else if selectedPlatform === 'android'}
                  <h4>{$t('remoteControlWizard.certificate.androidTitle')}</h4>
                  <ol class="platform-steps">
                    <li>{$t('remoteControlWizard.certificate.androidStep1')}</li>
                    <li>{$t('remoteControlWizard.certificate.androidStep2')}</li>
                    <li>{$t('remoteControlWizard.certificate.androidStep3')}</li>
                    <li>{$t('remoteControlWizard.certificate.androidStep4')}</li>
                    <li>{$t('remoteControlWizard.certificate.androidStep5')}</li>
                  </ol>
                {/if}
              </div>

              <div class="screenshot-placeholder">
                <span class="placeholder-text">{$t('remoteControlWizard.certificate.screenshot')}</span>
              </div>
            </div>

          {:else if currentStep === 'pair'}
            <div class="step-content">
              <div class="step-icon">
                <QrCode size={32} />
              </div>

              <p class="body-text">{$t('remoteControlWizard.pair.intro')}</p>

              <div class="method-section">
                <h4>{$t('remoteControlWizard.pair.qrMethod')}</h4>
                <ol class="step-list">
                  <li>{$t('remoteControlWizard.pair.qrStep1')}</li>
                  <li>{$t('remoteControlWizard.pair.qrStep2')}</li>
                  <li>{$t('remoteControlWizard.pair.qrStep3')}</li>
                </ol>
              </div>

              <div class="method-section">
                <h4>{$t('remoteControlWizard.pair.manualMethod')}</h4>
                <ol class="step-list">
                  <li>{$t('remoteControlWizard.pair.manualStep1')}</li>
                  <li>{$t('remoteControlWizard.pair.manualStep2')}</li>
                  <li>{$t('remoteControlWizard.pair.manualStep3')}</li>
                </ol>
              </div>

              <div class="troubleshoot-section">
                <h4>{$t('remoteControlWizard.pair.troubleshooting')}</h4>
                <ul class="trouble-list">
                  <li>{$t('remoteControlWizard.pair.trouble1')}</li>
                  <li>{$t('remoteControlWizard.pair.trouble2')}</li>
                  <li>{$t('remoteControlWizard.pair.trouble3')}</li>
                </ul>
              </div>

              <div class="screenshot-placeholder">
                <span class="placeholder-text">{$t('remoteControlWizard.pair.screenshot')}</span>
              </div>
            </div>

          {:else if currentStep === 'done'}
            <div class="done-content">
              <CheckCircle size={64} class="done-icon" />

              <p class="body-text pwa-hint">{$t('remoteControlWizard.done.pwaHint')}</p>

              <ol class="pwa-steps">
                <li>{$t('remoteControlWizard.done.pwaStep1')}</li>
                <li>{$t('remoteControlWizard.done.pwaStep2')}</li>
                <li>{$t('remoteControlWizard.done.pwaStep3')}</li>
              </ol>
            </div>
          {/if}
        </div>
      </div>

      <!-- Footer -->
      <footer class="wizard-footer">
        {#if currentStep === 'welcome'}
          <button class="btn btn-primary" onclick={next}>
            {$t('remoteControlWizard.welcome.start')}
          </button>
        {:else if currentStep === 'done'}
          <button class="btn btn-primary" onclick={handleClose}>
            {$t('remoteControlWizard.done.close')}
          </button>
        {:else}
          <button class="btn btn-secondary" onclick={back}>
            {$t('remoteControlWizard.buttons.back')}
          </button>
          <button class="btn btn-primary" onclick={next}>
            {$t('remoteControlWizard.buttons.next')}
          </button>
        {/if}
      </footer>
    </div>
  </div>
{/if}

<style>
  .wizard-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.7);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10000;
    padding: 24px;
    animation: fadeIn 150ms ease;
  }

  @keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  .wizard-modal {
    background: var(--bg-primary);
    border-radius: 12px;
    width: 100%;
    max-width: 800px;
    max-height: 90vh;
    display: flex;
    flex-direction: column;
    box-shadow: 0 24px 48px rgba(0, 0, 0, 0.4);
    border: 1px solid var(--border-subtle);
    animation: slideUp 200ms ease;
  }

  @keyframes slideUp {
    from {
      opacity: 0;
      transform: translateY(16px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  .wizard-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    padding: 24px;
    border-bottom: 1px solid var(--border-subtle);
    min-height: 64px;
  }

  .header-content h2 {
    font-size: 18px;
    font-weight: 700;
    color: var(--text-primary);
    margin: 0 0 4px 0;
  }

  .header-content p {
    font-size: 14px;
    color: var(--text-secondary);
    margin: 0;
  }

  .close-btn {
    width: 32px;
    height: 32px;
    border-radius: 8px;
    border: none;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: all 150ms ease;
  }

  .close-btn:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }

  .wizard-body {
    flex: 1;
    overflow-y: auto;
    padding: 24px;
    display: flex;
    gap: 24px;
  }

  .wizard-content {
    flex: 1;
    min-width: 0;
  }

  .step-content {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .welcome-content {
    align-items: center;
    text-align: center;
  }

  .welcome-icon {
    color: var(--accent-primary);
    margin-bottom: 8px;
  }

  .step-icon {
    color: var(--accent-primary);
    display: flex;
    justify-content: center;
    margin-bottom: 8px;
  }

  .body-text {
    font-size: 14px;
    color: var(--text-secondary);
    line-height: 1.6;
    margin: 0;
  }

  .intro-text {
    max-width: 500px;
  }

  .feature-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
    text-align: left;
    margin: 16px 0;
  }

  .feature-item {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 14px;
    color: var(--text-secondary);
  }

  .feature-item :global(.feature-check) {
    color: var(--accent-primary);
    flex-shrink: 0;
  }

  .screenshot-placeholder {
    width: 100%;
    max-width: 400px;
    height: 200px;
    background: var(--bg-tertiary);
    border: 2px dashed var(--border-subtle);
    border-radius: 8px;
    display: flex;
    align-items: center;
    justify-content: center;
    margin: 16px auto;
  }

  .placeholder-text {
    font-size: 12px;
    color: var(--text-muted);
    font-style: italic;
    text-align: center;
    padding: 16px;
  }

  .step-list {
    margin: 0;
    padding-left: 24px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .step-list li {
    font-size: 14px;
    color: var(--text-secondary);
    line-height: 1.5;
  }

  .note-text {
    font-size: 13px;
    color: var(--text-muted);
    background: var(--bg-secondary);
    padding: 12px 16px;
    border-radius: 8px;
    border-left: 3px solid var(--accent-primary);
  }

  .why-section,
  .method-section,
  .troubleshoot-section {
    margin-top: 16px;
    padding: 16px;
    background: var(--bg-secondary);
    border-radius: 8px;
  }

  .why-section h4,
  .method-section h4,
  .troubleshoot-section h4 {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0 0 12px 0;
  }

  .why-list,
  .trouble-list {
    margin: 0;
    padding-left: 20px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .why-list li,
  .trouble-list li {
    font-size: 13px;
    color: var(--text-secondary);
  }

  .platform-tabs {
    display: flex;
    gap: 8px;
    margin-top: 16px;
    flex-wrap: wrap;
  }

  .platform-tab {
    padding: 8px 16px;
    border-radius: 6px;
    border: 1px solid var(--border-subtle);
    background: var(--bg-secondary);
    color: var(--text-secondary);
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .platform-tab:hover {
    border-color: var(--accent-primary);
    color: var(--text-primary);
  }

  .platform-tab.active {
    background: var(--accent-primary);
    border-color: var(--accent-primary);
    color: white;
  }

  .platform-instructions {
    margin-top: 16px;
    padding: 16px;
    background: var(--bg-secondary);
    border-radius: 8px;
  }

  .platform-instructions h4 {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0 0 12px 0;
  }

  .platform-steps {
    margin: 0;
    padding-left: 20px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .platform-steps li {
    font-size: 13px;
    color: var(--text-secondary);
    line-height: 1.5;
  }

  .certificate-content {
    max-height: 60vh;
    overflow-y: auto;
  }

  .main-steps {
    background: var(--bg-secondary);
    padding: 16px;
    padding-left: 36px;
    border-radius: 8px;
  }

  .done-content {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    padding: 32px 0;
    gap: 24px;
  }

  .done-content :global(.done-icon) {
    color: var(--accent-primary);
  }

  .pwa-hint {
    font-size: 15px;
    color: var(--text-secondary);
    max-width: 400px;
  }

  .pwa-steps {
    text-align: left;
    margin: 0;
    padding-left: 24px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .pwa-steps li {
    font-size: 14px;
    color: var(--text-secondary);
  }

  .wizard-footer {
    display: flex;
    justify-content: flex-end;
    gap: 12px;
    padding: 16px 24px;
    border-top: 1px solid var(--border-subtle);
  }

  .btn {
    padding: 10px 20px;
    border-radius: 8px;
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: all 150ms ease;
  }

  .btn-primary {
    background: var(--accent-primary);
    color: white;
    border: none;
  }

  .btn-primary:hover {
    filter: brightness(1.1);
  }

  .btn-secondary {
    background: transparent;
    color: var(--text-secondary);
    border: 1px solid var(--border-subtle);
  }

  .btn-secondary:hover {
    background: var(--bg-tertiary);
    color: var(--text-primary);
  }
</style>
