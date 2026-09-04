"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.extractHost = extractHost;
/** Bounded declaration extraction; never imports/evaluates application modules. */
const node_fs_1 = __importDefault(require("node:fs"));
const node_path_1 = __importDefault(require("node:path"));
const typescript_parser_1 = __importDefault(require("typescript-parser"));
const SDK = "@lenso/cli/host";
function extractHost(filename) {
    const sources = new Map();
    const evaluating = new Set();
    let bytesRead = 0;
    let evaluations = 0;
    let stringBytes = 0;
    function fail(node, message) {
        const source = node.getSourceFile();
        const position = source.getLineAndCharacterOfPosition(node.getStart());
        throw new Error(`${source.fileName}:${position.line + 1}:${position.character + 1}: ${message}`);
    }
    function load(name) {
        const absolute = node_fs_1.default.realpathSync(name);
        const cached = sources.get(absolute);
        if (cached)
            return cached;
        const stat = node_fs_1.default.statSync(absolute);
        if (!stat.isFile() || stat.size > 256 * 1024 || sources.size >= 64) {
            throw new Error(`${absolute}: declaration source exceeds file/count limits`);
        }
        const content = node_fs_1.default.readFileSync(absolute, "utf8");
        bytesRead += Buffer.byteLength(content);
        if (bytesRead > 1024 * 1024)
            throw new Error("Host declarations exceed 1 MiB");
        const file = typescript_parser_1.default.createSourceFile(absolute, content, typescript_parser_1.default.ScriptTarget.Latest, true, typescript_parser_1.default.ScriptKind.TS);
        const diagnostics = file.parseDiagnostics;
        if (diagnostics.length) {
            throw new Error(`${absolute}: ${typescript_parser_1.default.flattenDiagnosticMessageText(diagnostics[0].messageText, " ")}`);
        }
        const source = { file, bindings: new Map() };
        sources.set(absolute, source);
        const add = (node, key, binding) => {
            if (source.bindings.has(key))
                fail(node, `duplicate declaration '${key}'`);
            source.bindings.set(key, binding);
        };
        for (const statement of file.statements) {
            if (typescript_parser_1.default.isImportDeclaration(statement)) {
                const clause = statement.importClause;
                if (!clause || !typescript_parser_1.default.isStringLiteral(statement.moduleSpecifier))
                    fail(statement, "only static declaration imports are supported");
                if (clause.isTypeOnly)
                    continue;
                const imported = statement.moduleSpecifier.text;
                if (clause.name)
                    add(clause.name, clause.name.text, { imported, exported: "default" });
                if (clause.namedBindings) {
                    if (!typescript_parser_1.default.isNamedImports(clause.namedBindings))
                        fail(statement, "namespace imports are not declaration references");
                    for (const item of clause.namedBindings.elements) {
                        if (!item.isTypeOnly)
                            add(item, item.name.text, { imported, exported: item.propertyName?.text ?? item.name.text });
                    }
                }
            }
            else if (typescript_parser_1.default.isVariableStatement(statement)) {
                if (!(statement.declarationList.flags & typescript_parser_1.default.NodeFlags.Const))
                    fail(statement, "Host declarations require const values");
                for (const item of statement.declarationList.declarations) {
                    if (!typescript_parser_1.default.isIdentifier(item.name) || !item.initializer)
                        fail(item, "expected an initialized const declaration");
                    add(item, item.name.text, { expression: item.initializer });
                }
            }
            else if (typescript_parser_1.default.isExportAssignment(statement) && !statement.isExportEquals) {
                if (source.output)
                    fail(statement, "duplicate default declaration");
                source.output = statement.expression;
            }
            else if (!typescript_parser_1.default.isInterfaceDeclaration(statement) && !typescript_parser_1.default.isTypeAliasDeclaration(statement) && !typescript_parser_1.default.isEmptyStatement(statement)) {
                fail(statement, "unsupported Host declaration statement; application code is never evaluated for metadata");
            }
        }
        return source;
    }
    function imported(binding, source, node) {
        if (!binding.imported.startsWith(".") || binding.exported !== "default") {
            fail(node, "expected a relative default declaration import; package references must be packed first");
        }
        const base = node_path_1.default.resolve(node_path_1.default.dirname(source.file.fileName), binding.imported);
        const candidates = node_path_1.default.extname(base) ? [base] : [base + ".ts", node_path_1.default.join(base, "index.ts")];
        const matches = candidates.filter(candidate => node_fs_1.default.existsSync(candidate));
        if (matches.length !== 1)
            fail(node, `declaration import '${binding.imported}' is missing or ambiguous`);
        const other = load(matches[0]);
        if (!other.output)
            fail(node, `declaration '${binding.imported}' has no default export`);
        return { expression: other.output, source: other };
    }
    function value(node, source, depth = 0) {
        if (++evaluations > 100_000 || depth > 64)
            fail(node, "declaration complexity limit exceeded");
        if (evaluating.has(node))
            fail(node, "cyclic declaration reference");
        evaluating.add(node);
        try {
            const next = (expression) => value(expression, source, depth + 1);
            if (typescript_parser_1.default.isStringLiteral(node) || typescript_parser_1.default.isNoSubstitutionTemplateLiteral(node)) {
                stringBytes += Buffer.byteLength(node.text);
                if (stringBytes > 4 * 1024 * 1024)
                    fail(node, "expanded declaration strings exceed 4 MiB");
                return node.text;
            }
            if (typescript_parser_1.default.isNumericLiteral(node)) {
                const number = Number(node.text);
                if (!Number.isFinite(number))
                    fail(node, "declaration numbers must be finite");
                return number;
            }
            if (node.kind === typescript_parser_1.default.SyntaxKind.TrueKeyword)
                return true;
            if (node.kind === typescript_parser_1.default.SyntaxKind.FalseKeyword)
                return false;
            if (node.kind === typescript_parser_1.default.SyntaxKind.NullKeyword)
                return null;
            if (typescript_parser_1.default.isParenthesizedExpression(node) || typescript_parser_1.default.isAsExpression(node) || typescript_parser_1.default.isSatisfiesExpression(node))
                return next(node.expression);
            if (typescript_parser_1.default.isPrefixUnaryExpression(node) && node.operator === typescript_parser_1.default.SyntaxKind.MinusToken && typescript_parser_1.default.isNumericLiteral(node.operand))
                return -next(node.operand);
            if (typescript_parser_1.default.isArrayLiteralExpression(node))
                return node.elements.map(next);
            if (typescript_parser_1.default.isObjectLiteralExpression(node)) {
                const result = Object.create(null);
                for (const property of node.properties) {
                    if ((!typescript_parser_1.default.isPropertyAssignment(property) && !typescript_parser_1.default.isShorthandPropertyAssignment(property)) || typescript_parser_1.default.isComputedPropertyName(property.name)) {
                        fail(property, "only literal properties and const shorthand are supported");
                    }
                    const key = property.name.text;
                    if (Object.hasOwn(result, key))
                        fail(property, `duplicate property '${key}'`);
                    result[key] = next(typescript_parser_1.default.isPropertyAssignment(property) ? property.initializer : property.name);
                }
                return result;
            }
            if (typescript_parser_1.default.isIdentifier(node)) {
                const binding = source.bindings.get(node.text);
                if (!binding)
                    fail(node, `unresolved declaration '${node.text}'`);
                if ("expression" in binding)
                    return next(binding.expression);
                const target = imported(binding, source, node);
                return value(target.expression, target.source, depth + 1);
            }
            if (typescript_parser_1.default.isCallExpression(node) && typescript_parser_1.default.isIdentifier(node.expression)) {
                const binding = source.bindings.get(node.expression.text);
                if (!binding || !("imported" in binding) || binding.imported !== SDK)
                    fail(node, "unsupported declaration call; do not execute application code to discover metadata");
                if (binding.exported === "defineHost" && node.arguments.length === 1)
                    return next(node.arguments[0]);
                if (binding.exported === "pluginBundle" && node.arguments.length >= 1 && node.arguments.length <= 2) {
                    const bundle = next(node.arguments[0]);
                    if (typeof bundle !== "string" || !bundle)
                        fail(node, "pluginBundle requires a literal bundle path");
                    const options = node.arguments[1] ? next(node.arguments[1]) : {};
                    if (!options || Array.isArray(options) || typeof options !== "object" || Object.keys(options).some(key => key !== "execution"))
                        fail(node, "pluginBundle accepts only an execution option");
                    const execution = options.execution ?? "bun";
                    if (execution !== "bun" && execution !== "process")
                        fail(node, "first Host authoring profile supports bun or process bundles");
                    const position = source.file.getLineAndCharacterOfPosition(node.getStart());
                    return { bundle: node_path_1.default.resolve(node_path_1.default.dirname(source.file.fileName), bundle), execution, source: `${source.file.fileName}:${position.line + 1}:${position.character + 1}` };
                }
            }
            fail(node, "unsupported declaration expression; use static declarations and packed Plugin references");
        }
        finally {
            evaluating.delete(node);
        }
    }
    const source = load(filename);
    const output = source.output;
    if (!output || !typescript_parser_1.default.isCallExpression(output) || !typescript_parser_1.default.isIdentifier(output.expression))
        throw new Error(`${filename}: expected export default defineHost({...})`);
    const factory = source.bindings.get(output.expression.text);
    if (!factory || !("imported" in factory) || factory.imported !== SDK || factory.exported !== "defineHost")
        fail(output, `Host must use defineHost from '${SDK}'`);
    return value(output, source);
}
if (require.main === module) {
    try {
        if (process.argv.length !== 3)
            throw new Error("expected one Host source path");
        process.stdout.write(JSON.stringify(extractHost(process.argv[2])));
    }
    catch (error) {
        console.error(error instanceof Error ? error.message : String(error));
        process.exitCode = 1;
    }
}
