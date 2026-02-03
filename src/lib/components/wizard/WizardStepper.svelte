<script lang="ts">
  import { Check } from 'lucide-svelte';

  export interface Step {
    id: string;
    label: string;
    status: 'complete' | 'active' | 'upcoming';
  }

  interface Props {
    steps: Step[];
    onStepClick?: (stepId: string) => void;
  }

  let { steps, onStepClick }: Props = $props();

  function handleClick(step: Step) {
    if (step.status === 'complete' && onStepClick) {
      onStepClick(step.id);
    }
  }
</script>

<nav class="stepper" aria-label="Progress">
  <ol class="step-list">
    {#each steps as step, index (step.id)}
      <li class="step-item">
        <button
          class="step-button"
          class:complete={step.status === 'complete'}
          class:active={step.status === 'active'}
          class:upcoming={step.status === 'upcoming'}
          class:clickable={step.status === 'complete'}
          onclick={() => handleClick(step)}
          disabled={step.status === 'upcoming'}
        >
          <span class="step-indicator">
            {#if step.status === 'complete'}
              <Check size={14} />
            {:else}
              <span class="step-number">{index + 1}</span>
            {/if}
          </span>
          <span class="step-label">{step.label}</span>
        </button>
        {#if index < steps.length - 1}
          <div class="step-connector" class:complete={step.status === 'complete'}></div>
        {/if}
      </li>
    {/each}
  </ol>
</nav>

<style>
  .stepper {
    min-width: 160px;
    flex-shrink: 0;
  }

  .step-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
  }

  .step-item {
    display: flex;
    flex-direction: column;
  }

  .step-button {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 0;
    background: none;
    border: none;
    cursor: default;
    text-align: left;
    width: 100%;
  }

  .step-button.clickable {
    cursor: pointer;
  }

  .step-button.clickable:hover .step-label {
    color: var(--text-primary);
  }

  .step-indicator {
    width: 24px;
    height: 24px;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 12px;
    font-weight: 600;
    flex-shrink: 0;
    transition: all 150ms ease;
  }

  .step-button.complete .step-indicator {
    background: var(--accent-primary);
    color: white;
  }

  .step-button.active .step-indicator {
    background: var(--accent-primary);
    color: white;
    box-shadow: 0 0 0 4px rgba(66, 133, 244, 0.2);
  }

  .step-button.upcoming .step-indicator {
    background: var(--bg-tertiary);
    color: var(--text-muted);
    border: 2px solid var(--border-subtle);
  }

  .step-number {
    font-size: 11px;
  }

  .step-label {
    font-size: 13px;
    color: var(--text-muted);
    transition: color 150ms ease;
  }

  .step-button.complete .step-label {
    color: var(--text-secondary);
  }

  .step-button.active .step-label {
    color: var(--text-primary);
    font-weight: 500;
  }

  .step-connector {
    width: 2px;
    height: 16px;
    background: var(--border-subtle);
    margin-left: 11px;
  }

  .step-connector.complete {
    background: var(--accent-primary);
  }
</style>
