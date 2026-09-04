"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.pluginBundle = pluginBundle;
exports.defineHost = defineHost;
function pluginBundle(bundle, options = {}) {
    return { bundle, execution: options.execution ?? "bun" };
}
function defineHost(declaration) {
    return declaration;
}
