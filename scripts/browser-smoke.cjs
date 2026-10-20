// Compatibility entry point.
require('./browser-matrix.cjs').main().catch(error=>{console.error(error);process.exitCode=1;});
