HumanGate is the mandatory authorisation card on any state-mutating AI proposal (NFR-AI-1): amber pending with Authorise/Reject, green with the audit id once authorised.

```jsx
<HumanGate title="Schedule maintenance · AC-2214" state={st} auditId="A-77120" onAuthorise={ok} onReject={no} />
```

Never auto-execute: the pending state is the default and the receipt (audit id) persists after resolution.