Table is the dense operator-console data table: mono ALL-CAPS headers on a sunken strip, hairline row dividers, accent left-rail on selection.

```jsx
<Table
  columns={[{header:"Dataset",key:"name"},{header:"Version",key:"v",mono:true},{header:"Status",cell:(r)=><StatusPill status={r.st} pulse={false}/>}]}
  rows={data} selectedIndex={sel} onRowClick={setSel} />
```