<script lang="ts">
  import { X, CheckCircle, Check, AlertTriangle, XCircle, Search, Loader2 } from 'lucide-svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { t } from '$lib/i18n';
  import WizardStepper, { type Step } from './wizard/WizardStepper.svelte';
  import CommandBlock from './wizard/CommandBlock.svelte';
  import WarningBanner from './wizard/WarningBanner.svelte';
  import DistroSelector from './wizard/DistroSelector.svelte';
  import BitPerfectAppSelector from './wizard/BitPerfectAppSelector.svelte';

  // DAC capabilities from Tauri backend
  interface DacCapabilities {
    node_name: string;
    sample_rates: number[];
    formats: string[];
    channels: number | null;
    description: string | null;
    error: string | null;
  }

  // DAC node name validation
  type DacType = 'usb' | 'pci' | 'bluetooth' | 'virtual' | 'unknown';
  type ValidationStatus = 'empty' | 'valid' | 'invalid';

  function validateNodeName(name: string): ValidationStatus {
    if (!name.trim()) return 'empty';
    // Valid patterns: alsa_output.* or alsa_input.*
    if (/^alsa_(output|input)\.[a-zA-Z0-9_.-]+$/.test(name)) return 'valid';
    // Might be valid but unusual format
    if (name.includes('alsa_output') || name.includes('alsa_input')) return 'valid';
    return 'invalid';
  }

  function detectDacType(name: string): DacType {
    const lower = name.toLowerCase();
    if (lower.includes('usb-') || lower.includes('.usb')) return 'usb';
    if (lower.includes('pci-') || lower.includes('.pci')) return 'pci';
    if (lower.includes('bluez') || lower.includes('bluetooth')) return 'bluetooth';
    if (lower.includes('virtual') || lower.includes('null') || lower.includes('dummy')) return 'virtual';
    return 'unknown';
  }

  interface Props {
    isOpen: boolean;
    onClose: () => void;
  }

  let { isOpen, onClose }: Props = $props();

  type WizardStep =
    | 'welcome'
    | 'precheck'
    | 'detect-dac'
    | 'backup'
    | 'pipewire-config'
    | 'pulse-config'
    | 'wireplumber-config'
    | 'restart'
    | 'verify'
    | 'done';

  const STEPS: WizardStep[] = [
    'welcome',
    'precheck',
    'detect-dac',
    'backup',
    'pipewire-config',
    'pulse-config',
    'wireplumber-config',
    'restart',
    'verify',
    'done'
  ];

  // Wizard state
  let currentStep = $state<WizardStep>('welcome');
  let completedSteps = $state(new Set<WizardStep>());
  let dacNodeName = $state('');
  let selectedApps = $state(['QBZ']);
  let welcomeConfirmed = $state(false);
  let precheckDone = $state(false);
  let backupConfirmed = $state(false);
  let restartDone = $state(false);
  let showRollback = $state(false);
  let selectedDistro = $state('debian');

  // DAC capabilities query state
  let dacCapabilities = $state<DacCapabilities | null>(null);
  let isQueryingDac = $state(false);

  // Sample rate selection for PipeWire config (step 5)
  const COMMON_SAMPLE_RATES = [44100, 48000, 88200, 96000, 176400, 192000];
  let selectedSampleRates = $state<Set<number>>(new Set());

  // Derive DAC short name for config filenames
  const dacShortName = $derived(() => {
    if (!dacNodeName) return 'dac';
    // Extract meaningful part from node name
    // alsa_output.usb-Cambridge_Audio_DacMagic_Plus-00.analog-stereo -> dacmagic-plus
    // alsa_output.usb-Generic_Macaron-00.analog-stereo -> macaron
    const match = dacNodeName.match(/usb-([^-]+)_([^-]+)/i);
    if (match) {
      return match[2].toLowerCase().replace(/_/g, '-');
    }
    // Fallback: try to get something from description
    if (dacCapabilities?.description) {
      return dacCapabilities.description.toLowerCase().replace(/\s+/g, '-').replace(/[^a-z0-9-]/g, '').slice(0, 20);
    }
    return 'dac';
  });

  async function queryDacCapabilities() {
    if (!dacNodeName.trim() || dacValidation !== 'valid') return;

    isQueryingDac = true;
    dacCapabilities = null;

    try {
      const caps = await invoke<DacCapabilities>('v2_query_dac_capabilities', {
        nodeName: dacNodeName
      });
      dacCapabilities = caps;

      // Pre-fill selected sample rates from detected capabilities
      if (caps.sample_rates.length > 0) {
        selectedSampleRates = new Set(caps.sample_rates.filter(r => COMMON_SAMPLE_RATES.includes(r)));
      }
    } catch (err) {
      console.error('Failed to query DAC capabilities:', err);
      dacCapabilities = {
        node_name: dacNodeName,
        sample_rates: [],
        formats: [],
        channels: null,
        description: null,
        error: String(err)
      };
    } finally {
      isQueryingDac = false;
    }
  }

  function toggleSampleRate(rate: number) {
    const newSet = new Set(selectedSampleRates);
    if (newSet.has(rate)) {
      newSet.delete(rate);
    } else {
      newSet.add(rate);
    }
    selectedSampleRates = newSet;
  }

  function generatePipewireConfig(): string[] {
    const rates = Array.from(selectedSampleRates).sort((a, b) => a - b);
    const ratesStr = rates.length > 0 ? rates.join(' ') : '44100 48000 88200 96000 176400 192000';
    const fileName = dacNodeName ? `99-qbz-dac-${dacShortName()}.conf` : '99-qbz-dac.conf';

    return [
      'mkdir -p ~/.config/pipewire/pipewire.conf.d',
      `cat > ~/.config/pipewire/pipewire.conf.d/${fileName} << 'EOF'`,
      '# QBZ DAC Setup - Sample Rate Switching',
      'context.properties = {',
      `  default.clock.allowed-rates = [ ${ratesStr} ]`,
      '}',
      'EOF'
    ];
  }

  // Reset state when modal opens
  $effect(() => {
    if (isOpen) {
      currentStep = 'welcome';
      completedSteps = new Set();
      dacNodeName = '';
      selectedApps = ['QBZ'];
      welcomeConfirmed = false;
      precheckDone = false;
      backupConfirmed = false;
      restartDone = false;
      showRollback = false;
      dacCapabilities = null;
      isQueryingDac = false;
      selectedSampleRates = new Set();
    }
  });

  // Derived values
  const currentIndex = $derived(STEPS.indexOf(currentStep));
  const dacValidation = $derived(validateNodeName(dacNodeName));
  const dacType = $derived(detectDacType(dacNodeName));

  // Convert kebab-case to camelCase for translation keys
  function toCamelCase(str: string): string {
    return str.replace(/-([a-z])/g, (_, letter) => letter.toUpperCase());
  }

  // Store labelKey instead of calling $t() in $derived - resolve in template
  const steps = $derived<(Step & { labelKey: string })[]>(STEPS.map((step, index) => ({
    id: step,
    label: step, // fallback
    labelKey: `dacWizard.steps.${toCamelCase(step)}`,
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

  // Generate pulse config command based on selected apps
  function generatePulseConfig(): string[] {
    const fileName = dacNodeName ? `99-qbz-bitperfect-${dacShortName()}.conf` : '99-qbz-bitperfect.conf';
    const rules = selectedApps.map(app => {
      const matchKey = app === 'QBZ' ? 'application.name' : 'application.process.binary';
      const matchValue = app === 'QBZ' ? 'QBZ' : app.toLowerCase();
      return `  {
    matches = [ { ${matchKey} = "${matchValue}" } ]
    actions = { update-props = { resample.disable = true, channelmix.disable = true } }
  }`;
    }).join('\n');

    return [
      'mkdir -p ~/.config/pipewire/pipewire-pulse.conf.d',
      `cat > ~/.config/pipewire/pipewire-pulse.conf.d/${fileName} << 'EOF'`,
      '# QBZ DAC Setup - Per-App Bit-Perfect',
      'pulse.rules = [',
      rules,
      ']',
      'EOF'
    ];
  }

  // Get created config file paths for summary
  function getCreatedConfigPaths(): string[] {
    const name = dacNodeName ? dacShortName() : null;
    return [
      `~/.config/pipewire/pipewire.conf.d/${name ? `99-qbz-dac-${name}.conf` : '99-qbz-dac.conf'}`,
      `~/.config/pipewire/pipewire-pulse.conf.d/${name ? `99-qbz-bitperfect-${name}.conf` : '99-qbz-bitperfect.conf'}`,
      `~/.config/wireplumber/wireplumber.conf.d/${name ? `99-qbz-dac-${name}.conf` : '99-qbz-dac.conf'}`
    ];
  }

  // Generate WirePlumber config with user's DAC node name
  function generateWireplumberConfig(): string[] {
    const rates = Array.from(selectedSampleRates).sort((a, b) => a - b);
    const ratesStr = rates.length > 0 ? rates.join(' ') : '44100 48000 88200 96000 176400 192000';
    const fileName = `99-qbz-dac-${dacShortName()}.conf`;
    const nodeName = dacNodeName || 'alsa_output.usb-YOUR_DAC-00.analog-stereo';

    return [
      'mkdir -p ~/.config/wireplumber/wireplumber.conf.d',
      `cat > ~/.config/wireplumber/wireplumber.conf.d/${fileName} << 'EOF'`,
      `# QBZ DAC Setup - ${dacCapabilities?.description || dacShortName()}`,
      'monitor.alsa.rules = [',
      '  {',
      '    matches = [',
      `      { node.name = "${nodeName}", media.class = "Audio/Sink" }`,
      '    ]',
      '    actions = {',
      '      update-props = {',
      `        audio.allowed-rates = [ ${ratesStr} ]`,
      '        resample.disable = true',
      '        channelmix.disable = true',
      '      }',
      '    }',
      '  }',
      ']',
      'EOF'
    ];
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
            <h2>{$t('dacWizard.welcome.title')}</h2>
            <p>{$t('dacWizard.welcome.subtitle')}</p>
          {:else if currentStep === 'precheck'}
            <h2>{$t('dacWizard.precheck.title')}</h2>
            <p>{$t('dacWizard.precheck.subtitle')}</p>
          {:else if currentStep === 'detect-dac'}
            <h2>{$t('dacWizard.detectDac.title')}</h2>
            <p>{$t('dacWizard.detectDac.subtitle')}</p>
          {:else if currentStep === 'backup'}
            <h2>{$t('dacWizard.backup.title')}</h2>
            <p>{$t('dacWizard.backup.subtitle')}</p>
          {:else if currentStep === 'pipewire-config'}
            <h2>{$t('dacWizard.pipewireConfig.title')}</h2>
            <p>{$t('dacWizard.pipewireConfig.subtitle')}</p>
          {:else if currentStep === 'pulse-config'}
            <h2>{$t('dacWizard.pulseConfig.title')}</h2>
            <p>{$t('dacWizard.pulseConfig.subtitle')}</p>
          {:else if currentStep === 'wireplumber-config'}
            <h2>{$t('dacWizard.wireplumberConfig.title')}</h2>
            <p>{$t('dacWizard.wireplumberConfig.subtitle')}</p>
          {:else if currentStep === 'restart'}
            <h2>{$t('dacWizard.restart.title')}</h2>
            <p>{$t('dacWizard.restart.subtitle')}</p>
          {:else if currentStep === 'verify'}
            <h2>{$t('dacWizard.verify.title')}</h2>
            <p>{$t('dacWizard.verify.subtitle')}</p>
          {:else if currentStep === 'done'}
            <h2>{$t('dacWizard.done.title')}</h2>
            <p>{$t('dacWizard.done.subtitle')}</p>
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
              <p class="body-text intro-text">{$t('dacWizard.welcome.intro')}</p>

              <div class="disclaimer-section">
                <p class="disclaimer-title">{$t('dacWizard.welcome.disclaimerTitle')}</p>
                <ul class="disclaimer-list">
                  <li>{$t('dacWizard.welcome.bulletResponsibility')}</li>
                  <li>
                    {$t('dacWizard.welcome.bulletNoGuarantee')}
                    <ul class="system-issues">
                      <li>{$t('dacWizard.welcome.systemIssue1')}</li>
                      <li>{$t('dacWizard.welcome.systemIssue2')}</li>
                      <li>{$t('dacWizard.welcome.systemIssue3')}</li>
                    </ul>
                  </li>
                  <li>{$t('dacWizard.welcome.bulletHelper')}</li>
                </ul>
              </div>

              <p class="body-text recovery-text">{$t('dacWizard.welcome.recovery')}</p>

              <p class="body-text ready-text">{$t('dacWizard.welcome.readyText')}</p>

              <label class="checkbox-row">
                <input type="checkbox" bind:checked={welcomeConfirmed} />
                <span>{$t('dacWizard.welcome.checkbox')}</span>
              </label>
            </div>

          {:else if currentStep === 'precheck'}
            <div class="step-content">
              <CommandBlock
                label={$t('dacWizard.precheck.hint')}
                command="systemctl --user status pipewire pipewire-pulse wireplumber"
              />

              <p class="inactive-warning">{$t('dacWizard.precheck.inactiveWarning')}</p>

              <label class="checkbox-row">
                <input type="checkbox" bind:checked={precheckDone} />
                <span>{$t('dacWizard.precheck.checkbox')}</span>
              </label>

              {#if !precheckDone}
                <div class="install-section">
                  <h4>{$t('dacWizard.precheck.installTitle')}</h4>
                  <p class="install-subtitle">{$t('dacWizard.precheck.installSubtitle')}</p>
                  <DistroSelector bind:selected={selectedDistro} />
                </div>
              {/if}
            </div>

          {:else if currentStep === 'detect-dac'}
            <div class="step-content">
              <CommandBlock
                label={$t('dacWizard.detectDac.step1')}
                command="wpctl status"
              />

              <CommandBlock
                label={$t('dacWizard.detectDac.step2')}
                command="wpctl inspect <ID> | grep node.name"
              />

              <div class="input-group">
                <label class="input-label">{$t('dacWizard.detectDac.inputLabel')}</label>
                <p class="input-warning">{$t('dacWizard.detectDac.inputWarning')}</p>
                <input
                  type="text"
                  class="text-input mono"
                  class:valid={dacValidation === 'valid'}
                  class:invalid={dacValidation === 'invalid'}
                  bind:value={dacNodeName}
                  placeholder={$t('dacWizard.detectDac.inputPlaceholder')}
                />

                {#if dacValidation === 'valid'}
                  <div class="validation-feedback">
                    <span class="validation-status valid">
                      <Check size={14} />
                      {$t('dacWizard.detectDac.validation.validFormat')}
                    </span>

                    {#if dacType === 'usb'}
                      <span class="dac-type usb">
                        <CheckCircle size={14} />
                        {$t('dacWizard.detectDac.validation.usbDac')}
                      </span>
                    {:else if dacType === 'pci'}
                      <span class="dac-type pci">
                        <AlertTriangle size={14} />
                        {$t('dacWizard.detectDac.validation.pciDac')}
                      </span>
                    {:else if dacType === 'bluetooth'}
                      <span class="dac-type bluetooth">
                        <XCircle size={14} />
                        {$t('dacWizard.detectDac.validation.bluetoothDac')}
                      </span>
                    {:else if dacType === 'virtual'}
                      <span class="dac-type virtual">
                        <XCircle size={14} />
                        {$t('dacWizard.detectDac.validation.virtualDac')}
                      </span>
                    {/if}
                  </div>
                {:else if dacValidation === 'invalid'}
                  <div class="validation-feedback">
                    <span class="validation-status invalid">
                      <XCircle size={14} />
                      {$t('dacWizard.detectDac.validation.invalidFormat')}
                    </span>
                  </div>
                {/if}
              </div>

              <!-- Query DAC Capabilities -->
              {#if dacValidation === 'valid'}
                <div class="query-section">
                  <button
                    class="query-btn"
                    onclick={queryDacCapabilities}
                    disabled={isQueryingDac}
                  >
                    {#if isQueryingDac}
                      <Loader2 size={16} class="spin" />
                      {$t('dacWizard.detectDac.query.querying')}
                    {:else}
                      <Search size={16} />
                      {$t('dacWizard.detectDac.query.button')}
                    {/if}
                  </button>

                  {#if dacCapabilities}
                    <div class="capabilities-result">
                      {#if dacCapabilities.description}
                        <div class="cap-row">
                          <span class="cap-label">{$t('dacWizard.detectDac.query.device')}:</span>
                          <span class="cap-value">{dacCapabilities.description}</span>
                        </div>
                      {/if}

                      {#if dacCapabilities.sample_rates.length > 0}
                        <div class="cap-row">
                          <span class="cap-label">{$t('dacWizard.detectDac.query.sampleRates')}:</span>
                          <span class="cap-value rates">
                            {dacCapabilities.sample_rates.map(r => `${(r / 1000).toFixed(1)}kHz`).join(', ')}
                          </span>
                        </div>
                      {/if}

                      {#if dacCapabilities.formats.length > 0}
                        <div class="cap-row">
                          <span class="cap-label">{$t('dacWizard.detectDac.query.formats')}:</span>
                          <span class="cap-value">{dacCapabilities.formats.join(', ')}</span>
                        </div>
                      {/if}

                      {#if dacCapabilities.channels}
                        <div class="cap-row">
                          <span class="cap-label">{$t('dacWizard.detectDac.query.channels')}:</span>
                          <span class="cap-value">{dacCapabilities.channels}</span>
                        </div>
                      {/if}

                      {#if dacCapabilities.error}
                        <WarningBanner
                          variant="warning"
                          body={dacCapabilities.error}
                        />
                      {:else}
                        <WarningBanner
                          variant="info"
                          body={$t('dacWizard.detectDac.query.disclaimer')}
                        />
                      {/if}
                    </div>
                  {/if}
                </div>
              {/if}
            </div>

          {:else if currentStep === 'backup'}
            <div class="step-content">
              <CommandBlock
                command={[
                  'BACKUP=~/.config/qbz/backups/pipewire-$(date +%Y%m%d-%H%M%S)',
                  'mkdir -p "$BACKUP"',
                  'cp -a ~/.config/pipewire "$BACKUP/" 2>/dev/null || true',
                  'cp -a ~/.config/wireplumber "$BACKUP/" 2>/dev/null || true',
                  'echo "Backup created at: $BACKUP"'
                ]}
              />

              <WarningBanner variant="info" body={$t('dacWizard.backup.hint')} />

              <label class="checkbox-row">
                <input type="checkbox" bind:checked={backupConfirmed} />
                <span>{$t('dacWizard.backup.checkbox')}</span>
              </label>
            </div>

          {:else if currentStep === 'pipewire-config'}
            <div class="step-content">
              <p class="body-text">{$t('dacWizard.pipewireConfig.explanation')}</p>

              <div class="sample-rate-selector">
                <label class="selector-label">{$t('dacWizard.pipewireConfig.selectRates')}</label>
                <div class="rate-checkboxes">
                  {#each COMMON_SAMPLE_RATES as rate}
                    <label class="rate-checkbox" class:detected={dacCapabilities?.sample_rates.includes(rate)}>
                      <input
                        type="checkbox"
                        checked={selectedSampleRates.has(rate)}
                        onchange={() => toggleSampleRate(rate)}
                      />
                      <span>{(rate / 1000).toFixed(1)}kHz</span>
                      {#if dacCapabilities?.sample_rates.includes(rate)}
                        <span class="detected-badge">detected</span>
                      {/if}
                    </label>
                  {/each}
                </div>
                {#if selectedSampleRates.size === 0}
                  <p class="rate-hint">{$t('dacWizard.pipewireConfig.noRatesHint')}</p>
                {/if}
              </div>

              <CommandBlock command={generatePipewireConfig()} />
            </div>

          {:else if currentStep === 'pulse-config'}
            <div class="step-content">
              <BitPerfectAppSelector bind:selectedApps />

              <WarningBanner variant="warning" body={$t('dacWizard.pulseConfig.warning')} />

              <CommandBlock command={generatePulseConfig()} />
            </div>

          {:else if currentStep === 'wireplumber-config'}
            <div class="step-content">
              <div class="targeting-info">
                <span class="targeting-label">{$t('dacWizard.wireplumberConfig.targeting')}</span>
                <code class="targeting-value">{dacNodeName}</code>
              </div>

              <p class="rules-note">{$t('dacWizard.wireplumberConfig.rulesNote')}</p>

              <CommandBlock command={generateWireplumberConfig()} />
            </div>

          {:else if currentStep === 'restart'}
            <div class="step-content">
              <CommandBlock
                command="systemctl --user restart pipewire pipewire-pulse wireplumber"
              />

              <WarningBanner variant="info" body={$t('dacWizard.restart.hint')} />

              <label class="checkbox-row">
                <input type="checkbox" bind:checked={restartDone} />
                <span>{$t('dacWizard.restart.checkbox')}</span>
              </label>
            </div>

          {:else if currentStep === 'verify'}
            <div class="step-content">
              <div class="verify-instructions">
                <pre>{$t('dacWizard.verify.instructions')}</pre>
              </div>

              <CommandBlock command="pw-top" />

              <p class="success-hint">{$t('dacWizard.verify.success')}</p>

              {#if showRollback}
                <div class="rollback-section">
                  <h4>{$t('dacWizard.verify.rollbackTitle')}</h4>
                  <p class="rollback-hint">{$t('dacWizard.verify.rollbackHint')}</p>
                  <CommandBlock
                    command={[
                      '# Restore backup',
                      'BACKUP=$(ls -td ~/.config/qbz/backups/pipewire-* | head -1)',
                      'rm -rf ~/.config/pipewire ~/.config/wireplumber',
                      'cp -a "$BACKUP/pipewire" ~/.config/',
                      'cp -a "$BACKUP/wireplumber" ~/.config/',
                      'systemctl --user restart pipewire pipewire-pulse wireplumber'
                    ]}
                  />

                  <WarningBanner
                    variant="info"
                    title={$t('dacWizard.error.title')}
                    body={$t('dacWizard.error.body')}
                    links={[
                      { label: $t('dacWizard.error.pipewireDocs'), url: $t('dacWizard.error.pipewireUrl') },
                      { label: $t('dacWizard.error.archWiki'), url: $t('dacWizard.error.archWikiUrl') }
                    ]}
                  />
                </div>
              {/if}
            </div>

          {:else if currentStep === 'done'}
            <div class="done-content">
              <CheckCircle size={64} class="done-icon" />

              {#if dacCapabilities?.description || dacNodeName}
                <div class="done-dac-info">
                  <span class="done-dac-label">{$t('dacWizard.done.configuredFor')}</span>
                  <span class="done-dac-name">{dacCapabilities?.description || dacShortName()}</span>
                </div>
              {/if}

              <div class="done-summary">
                <h4>{$t('dacWizard.done.summary')}</h4>
                <ul class="config-list">
                  {#each getCreatedConfigPaths() as path}
                    <li><code>{path}</code></li>
                  {/each}
                </ul>
              </div>
            </div>
          {/if}
        </div>
      </div>

      <!-- Footer -->
      <footer class="wizard-footer">
        {#if currentStep === 'welcome'}
          <button class="btn btn-primary" onclick={next} disabled={!welcomeConfirmed}>
            {$t('dacWizard.welcome.start')}
          </button>
        {:else if currentStep === 'verify'}
          <button class="btn btn-secondary" onclick={() => { showRollback = true; }}>
            {$t('dacWizard.verify.failed')}
          </button>
          <button class="btn btn-ghost" onclick={next}>
            {$t('dacWizard.verify.skip')}
          </button>
          <button class="btn btn-primary" onclick={next}>
            {$t('dacWizard.verify.passed')}
          </button>
        {:else if currentStep === 'done'}
          <button class="btn btn-primary" onclick={handleClose}>
            {$t('dacWizard.done.close')}
          </button>
        {:else}
          <button class="btn btn-secondary" onclick={back}>
            {$t('dacWizard.buttons.back')}
          </button>
          <button
            class="btn btn-primary"
            onclick={next}
            disabled={
              (currentStep === 'precheck' && !precheckDone) ||
              (currentStep === 'backup' && !backupConfirmed) ||
              (currentStep === 'restart' && !restartDone)
            }
          >
            {$t('dacWizard.buttons.next')}
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
    max-width: 860px;
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
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-tertiary);
    border: none;
    border-radius: 6px;
    color: var(--text-muted);
    cursor: pointer;
    transition: all 150ms ease;
    flex-shrink: 0;
  }

  .close-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .wizard-body {
    flex: 1;
    overflow-y: auto;
    display: flex;
    gap: 24px;
    padding: 24px;
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

  .body-text {
    font-size: 14px;
    color: var(--text-secondary);
    line-height: 1.6;
    margin: 0;
    white-space: pre-line;
  }

  /* Welcome step styles */
  .welcome-content {
    gap: 20px;
  }

  .intro-text {
    color: var(--text-primary);
    font-weight: 500;
  }

  .disclaimer-section {
    background: var(--bg-tertiary);
    border-radius: 8px;
    padding: 16px;
    border-left: 3px solid var(--warning, #fbbf24);
  }

  .disclaimer-title {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0 0 12px 0;
  }

  .disclaimer-list {
    margin: 0;
    padding-left: 20px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .disclaimer-list > li {
    font-size: 13px;
    color: var(--text-secondary);
    line-height: 1.5;
  }

  .system-issues {
    margin: 8px 0 0 0;
    padding-left: 16px;
    list-style-type: disc;
  }

  .system-issues li {
    font-size: 12px;
    color: var(--text-muted);
    line-height: 1.4;
  }

  .recovery-text {
    padding: 12px;
    background: rgba(34, 197, 94, 0.1);
    border-radius: 6px;
    border-left: 3px solid var(--color-success, #22c55e);
  }

  .ready-text {
    font-style: italic;
    color: var(--text-muted);
  }

  /* Micro-adjustment warning texts */
  .inactive-warning {
    font-size: 13px;
    color: var(--warning, #fbbf24);
    margin: 0;
    padding: 8px 12px;
    background: rgba(251, 191, 36, 0.1);
    border-radius: 6px;
    border-left: 3px solid var(--warning, #fbbf24);
  }

  .input-warning {
    font-size: 12px;
    color: var(--text-muted);
    margin: 0;
    font-style: italic;
  }

  .rules-note {
    font-size: 13px;
    color: var(--color-success, #22c55e);
    margin: 0;
    padding: 8px 12px;
    background: rgba(34, 197, 94, 0.1);
    border-radius: 6px;
  }

  .checkbox-row {
    display: flex;
    align-items: center;
    gap: 10px;
    cursor: pointer;
    padding: 10px 12px;
    background: var(--bg-tertiary);
    border-radius: 6px;
    transition: background-color 150ms ease;
  }

  .checkbox-row:hover {
    background: var(--bg-hover);
  }

  .checkbox-row input {
    accent-color: var(--accent-primary);
    width: 16px;
    height: 16px;
  }

  .checkbox-row span {
    font-size: 14px;
    color: var(--text-primary);
  }

  .install-section {
    margin-top: 16px;
    padding-top: 16px;
    border-top: 1px solid var(--border-subtle);
  }

  .install-section h4 {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0 0 8px 0;
  }

  .install-subtitle {
    font-size: 13px;
    color: var(--text-secondary);
    margin: 0 0 12px 0;
  }

  .input-group {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .input-label {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-secondary);
  }

  .text-input {
    padding: 10px 12px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    font-size: 14px;
    color: var(--text-primary);
    transition: border-color 150ms ease;
  }

  .text-input:focus {
    outline: none;
    border-color: var(--accent-primary);
  }

  .text-input.mono {
    font-family: var(--font-mono, monospace);
  }

  .text-input::placeholder {
    color: var(--text-muted);
  }

  .text-input.valid {
    border-color: var(--color-success, #22c55e);
  }

  .text-input.invalid {
    border-color: var(--color-error, #ef4444);
  }

  .validation-feedback {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 4px;
  }

  .validation-status {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
  }

  .validation-status.valid {
    color: var(--color-success, #22c55e);
  }

  .validation-status.invalid {
    color: var(--color-error, #ef4444);
  }

  .dac-type {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    padding: 6px 10px;
    border-radius: 6px;
  }

  .dac-type.usb {
    color: var(--color-success, #22c55e);
    background: rgba(34, 197, 94, 0.1);
  }

  .dac-type.pci {
    color: var(--warning, #fbbf24);
    background: rgba(251, 191, 36, 0.1);
  }

  .dac-type.bluetooth,
  .dac-type.virtual {
    color: var(--color-error, #ef4444);
    background: rgba(239, 68, 68, 0.1);
  }

  .query-section {
    display: flex;
    flex-direction: column;
    gap: 12px;
    margin-top: 8px;
  }

  .query-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 10px 16px;
    background: var(--accent-primary);
    border: none;
    border-radius: 6px;
    color: white;
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: opacity 150ms ease;
  }

  .query-btn:hover:not(:disabled) {
    opacity: 0.9;
  }

  .query-btn:disabled {
    opacity: 0.7;
    cursor: not-allowed;
  }

  .query-btn :global(.spin) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  .capabilities-result {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px;
    background: var(--bg-tertiary);
    border-radius: 6px;
  }

  .cap-row {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .cap-label {
    font-size: 12px;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .cap-value {
    font-size: 14px;
    color: var(--text-primary);
  }

  .cap-value.rates {
    font-family: var(--font-mono, monospace);
    color: var(--color-success, #22c55e);
  }

  .sample-rate-selector {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin-bottom: 12px;
  }

  .selector-label {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-secondary);
  }

  .rate-checkboxes {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .rate-checkbox {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 12px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    cursor: pointer;
    transition: all 150ms ease;
    font-size: 13px;
    color: var(--text-primary);
  }

  .rate-checkbox:hover {
    background: var(--bg-hover);
  }

  .rate-checkbox:has(input:checked) {
    border-color: var(--accent-primary);
    background: rgba(66, 133, 244, 0.1);
  }

  .rate-checkbox.detected {
    border-color: var(--color-success, #22c55e);
  }

  .rate-checkbox.detected:has(input:checked) {
    border-color: var(--color-success, #22c55e);
    background: rgba(34, 197, 94, 0.1);
  }

  .rate-checkbox input {
    accent-color: var(--accent-primary);
  }

  .rate-checkbox.detected input {
    accent-color: var(--color-success, #22c55e);
  }

  .detected-badge {
    font-size: 10px;
    padding: 2px 6px;
    background: var(--color-success, #22c55e);
    color: white;
    border-radius: 4px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .rate-hint {
    font-size: 12px;
    color: var(--text-muted);
    font-style: italic;
  }

  .targeting-info {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 12px;
    background: var(--bg-tertiary);
    border-radius: 6px;
  }

  .targeting-label {
    font-size: 13px;
    color: var(--text-secondary);
  }

  .targeting-value {
    font-size: 14px;
    font-family: var(--font-mono, monospace);
    color: var(--accent-primary);
    word-break: break-all;
  }

  .verify-instructions {
    padding: 12px;
    background: var(--bg-tertiary);
    border-radius: 6px;
  }

  .verify-instructions pre {
    margin: 0;
    font-size: 13px;
    color: var(--text-secondary);
    line-height: 1.6;
    white-space: pre-wrap;
  }

  .success-hint {
    font-size: 14px;
    color: var(--text-secondary);
    margin: 0;
    font-style: italic;
  }

  .rollback-section {
    margin-top: 16px;
    padding-top: 16px;
    border-top: 1px solid var(--border-subtle);
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .rollback-section h4 {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0;
  }

  .rollback-hint {
    font-size: 13px;
    color: var(--text-secondary);
    margin: 0;
  }

  .done-content {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 24px;
    padding: 40px 0;
  }

  .done-content :global(.done-icon) {
    color: var(--accent-primary);
  }

  .done-dac-info {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
  }

  .done-dac-label {
    font-size: 13px;
    color: var(--text-muted);
  }

  .done-dac-name {
    font-size: 18px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .done-summary {
    text-align: center;
  }

  .done-summary h4 {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-primary);
    margin: 0 0 12px 0;
  }

  .config-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .config-list code {
    font-size: 12px;
    font-family: var(--font-mono, monospace);
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
    border: none;
  }

  .btn-primary {
    background: var(--accent-primary);
    color: white;
  }

  .btn-primary:hover:not(:disabled) {
    opacity: 0.9;
  }

  .btn-primary:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn-secondary {
    background: var(--bg-tertiary);
    color: var(--text-primary);
    border: 1px solid var(--border-subtle);
  }

  .btn-secondary:hover {
    background: var(--bg-hover);
  }

  .btn-ghost {
    background: transparent;
    color: var(--text-secondary);
  }

  .btn-ghost:hover {
    color: var(--text-primary);
    background: var(--alpha-8, rgba(255,255,255,0.08));
  }

  /* Responsive */
  @media (max-width: 700px) {
    .wizard-modal {
      max-width: 100%;
      max-height: 100vh;
      border-radius: 0;
    }

    .wizard-body {
      flex-direction: column;
    }

    .wizard-footer {
      flex-direction: column;
    }

    .btn {
      width: 100%;
    }
  }
</style>
