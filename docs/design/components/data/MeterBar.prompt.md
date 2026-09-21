MeterBar is the console meter: CPU, pass rate, classification confidence, ring progress.

```jsx
<MeterBar value={88} warnAt={85} label="CPU" />
<MeterBar value={54} warnAt={70} invert label="CONF." />
```

Green when safe; warn/danger past thresholds; invert for lower-is-worse metrics.