Button is the action primitive: accent-filled primary (max one per view), ink secondary, accent outline, and bare ghost.

```jsx
<Button variant="primary" iconRight={<Icon name="arrow-right" size={16} />}>Deploy release</Button>
<Button variant="secondary" size="sm">Cancel</Button>
```

Sizes sm/md/lg (30/38/48px). Primary glows (--shadow-glow) on hover; press nudges 1px down. The accent follows the active .theme-* scope.