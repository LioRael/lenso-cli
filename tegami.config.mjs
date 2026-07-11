export default {
  groups: {
    "lenso-cli": {
      syncBump: true,
      syncGitTag: true,
    },
  },
  packages: {
    "cargo:lenso-cli": { group: "lenso-cli" },
    "@lenso/cli": { group: "lenso-cli" },
  },
  npm: { bumpDep: () => false },
};
