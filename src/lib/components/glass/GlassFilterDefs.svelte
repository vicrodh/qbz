<script lang="ts">
  interface Props {
    id?: string;
    baseFrequency?: string;
    numOctaves?: number;
    seed?: number;
    scale?: number;
  }

  let {
    id = 'glass-dist',
    baseFrequency = '0.005 0.005',
    numOctaves = 5,
    seed = 92,
    scale = 80
  }: Props = $props();
</script>

<svg class="glass-defs" aria-hidden="true">
  <filter id={id} x="0%" y="0%" width="100%" height="100%">
    <feTurbulence
      type="fractalNoise"
      baseFrequency={baseFrequency}
      numOctaves={numOctaves}
      seed={seed}
      result="noise"
    />
    <feGaussianBlur in="noise" stdDeviation="2" result="blurred" />
    <feDisplacementMap
      in="SourceGraphic"
      in2="blurred"
      scale={scale}
      xChannelSelector="R"
      yChannelSelector="G"
    />
  </filter>
</svg>

<style>
  .glass-defs {
    position: absolute;
    width: 0;
    height: 0;
    overflow: hidden;
  }
</style>
