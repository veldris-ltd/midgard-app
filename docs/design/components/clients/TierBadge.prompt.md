TierBadge marks every client AI result with the tier that served it (FR-C-6): on-device / remote / hybrid / disabled, plus whether data left the device.

```jsx
<TierBadge tier="on-device" model="GEMMA INT4" leaves={false} />
<TierBadge tier="remote" model="Mistral Small 3.2" />
```