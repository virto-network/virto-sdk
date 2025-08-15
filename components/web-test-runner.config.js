import { summaryReporter } from '@web/test-runner';

export default {
  files: ['virto-**/*.test.js'],
  nodeResolve: true,
  reporters: [
    summaryReporter()
  ],
};
