import { writeFileSync, renameSync } from "node:fs";
import { launchOwnedProcess } from "../../bin/host-owner.js";

const app = await launchOwnedProcess(JSON.parse(process.env.LENSO_OWNER_TEST_OPTIONS));
const output = process.env.LENSO_OWNER_TEST_OUTPUT;
writeFileSync(`${output}.tmp`, JSON.stringify({ pid: app.pid }));
renameSync(`${output}.tmp`, output);
await app.closed;
