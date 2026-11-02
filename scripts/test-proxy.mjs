import { cargo, run } from './native.mjs';
run(cargo, ['test', '--locked', '--lib', 'proxy']);
