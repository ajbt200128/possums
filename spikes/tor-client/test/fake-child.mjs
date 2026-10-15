// Test-only child: IPC fixtures, no Tor imports, bootstrap or network access.
const behavior = process.env.FIXTURE_BEHAVIOR;
process.send?.({ progress: 10 });
if (behavior === 'exit') process.exit(1);
if (behavior !== 'stall') setTimeout(() => process.send?.({ progress: 100 }), 5);
setInterval(() => {}, 1000);
