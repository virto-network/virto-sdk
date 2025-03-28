module.exports = {
  launch: {
    headless: true,
    args: ['--no-sandbox', '--disable-setuid-sandbox'],
  },
  server: {
    command: 'npm run dev',
    port: 3000,
    launchTimeout: 10000,
  },
};
