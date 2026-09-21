Select is the dropdown field, matching Input's ink-well look.

```jsx
<Select label="Release channel" options={["stable","beta","canary"]} value={ch} onChange={(e)=>setCh(e.target.value)} />
```