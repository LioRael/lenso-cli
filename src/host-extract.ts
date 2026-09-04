/** Bounded declaration extraction; never imports/evaluates application modules. */
import fs from "node:fs";
import path from "node:path";
import ts from "typescript-parser";

type Data = null | boolean | number | string | Data[] | { [key: string]: Data };
type Binding = { expression: ts.Expression } | { imported: string; exported: string };
interface Source {
  file: ts.SourceFile;
  bindings: Map<string, Binding>;
  output?: ts.Expression;
}
const SDK = "@lenso/cli/host";

export function extractHost(filename: string): Data {
  const sources = new Map<string, Source>();
  const evaluating = new Set<ts.Node>();
  let bytesRead = 0;
  let evaluations = 0;
  let stringBytes = 0;

  function fail(node: ts.Node, message: string): never {
    const source = node.getSourceFile();
    const position = source.getLineAndCharacterOfPosition(node.getStart());
    throw new Error(`${source.fileName}:${position.line + 1}:${position.character + 1}: ${message}`);
  }
  function load(name: string): Source {
    const absolute = fs.realpathSync(name);
    const cached = sources.get(absolute);
    if (cached) return cached;
    const stat = fs.statSync(absolute);
    if (!stat.isFile() || stat.size > 256 * 1024 || sources.size >= 64) {
      throw new Error(`${absolute}: declaration source exceeds file/count limits`);
    }
    const content = fs.readFileSync(absolute, "utf8");
    bytesRead += Buffer.byteLength(content);
    if (bytesRead > 1024 * 1024) throw new Error("Host declarations exceed 1 MiB");
    const file = ts.createSourceFile(absolute, content, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
    const diagnostics = (file as ts.SourceFile & { parseDiagnostics: readonly ts.Diagnostic[] }).parseDiagnostics;
    if (diagnostics.length) {
      throw new Error(`${absolute}: ${ts.flattenDiagnosticMessageText(diagnostics[0].messageText, " ")}`);
    }
    const source: Source = { file, bindings: new Map() };
    sources.set(absolute, source);
    const add = (node: ts.Node, key: string, binding: Binding): void => {
      if (source.bindings.has(key)) fail(node, `duplicate declaration '${key}'`);
      source.bindings.set(key, binding);
    };
    for (const statement of file.statements) {
      if (ts.isImportDeclaration(statement)) {
        const clause = statement.importClause;
        if (!clause || !ts.isStringLiteral(statement.moduleSpecifier)) fail(statement, "only static declaration imports are supported");
        if (clause.isTypeOnly) continue;
        const imported = statement.moduleSpecifier.text;
        if (clause.name) add(clause.name, clause.name.text, { imported, exported: "default" });
        if (clause.namedBindings) {
          if (!ts.isNamedImports(clause.namedBindings)) fail(statement, "namespace imports are not declaration references");
          for (const item of clause.namedBindings.elements) {
            if (!item.isTypeOnly) add(item, item.name.text, { imported, exported: item.propertyName?.text ?? item.name.text });
          }
        }
      } else if (ts.isVariableStatement(statement)) {
        if (!(statement.declarationList.flags & ts.NodeFlags.Const)) fail(statement, "Host declarations require const values");
        for (const item of statement.declarationList.declarations) {
          if (!ts.isIdentifier(item.name) || !item.initializer) fail(item, "expected an initialized const declaration");
          add(item, item.name.text, { expression: item.initializer });
        }
      } else if (ts.isExportAssignment(statement) && !statement.isExportEquals) {
        if (source.output) fail(statement, "duplicate default declaration");
        source.output = statement.expression;
      } else if (!ts.isInterfaceDeclaration(statement) && !ts.isTypeAliasDeclaration(statement) && !ts.isEmptyStatement(statement)) {
        fail(statement, "unsupported Host declaration statement; application code is never evaluated for metadata");
      }
    }
    return source;
  }

  function imported(binding: { imported: string; exported: string }, source: Source, node: ts.Node): { expression: ts.Expression; source: Source } {
    if (!binding.imported.startsWith(".") || binding.exported !== "default") {
      fail(node, "expected a relative default declaration import; package references must be packed first");
    }
    const base = path.resolve(path.dirname(source.file.fileName), binding.imported);
    const candidates = path.extname(base) ? [base] : [base + ".ts", path.join(base, "index.ts")];
    const matches = candidates.filter(candidate => fs.existsSync(candidate));
    if (matches.length !== 1) fail(node, `declaration import '${binding.imported}' is missing or ambiguous`);
    const other = load(matches[0]);
    if (!other.output) fail(node, `declaration '${binding.imported}' has no default export`);
    return { expression: other.output, source: other };
  }

  function value(node: ts.Expression, source: Source, depth = 0): Data {
    if (++evaluations > 100_000 || depth > 64) fail(node, "declaration complexity limit exceeded");
    if (evaluating.has(node)) fail(node, "cyclic declaration reference");
    evaluating.add(node);
    try {
      const next = (expression: ts.Expression): Data => value(expression, source, depth + 1);
      if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) {
        stringBytes += Buffer.byteLength(node.text);
        if (stringBytes > 4 * 1024 * 1024) fail(node, "expanded declaration strings exceed 4 MiB");
        return node.text;
      }
      if (ts.isNumericLiteral(node)) {
        const number = Number(node.text);
        if (!Number.isFinite(number)) fail(node, "declaration numbers must be finite");
        return number;
      }
      if (node.kind === ts.SyntaxKind.TrueKeyword) return true;
      if (node.kind === ts.SyntaxKind.FalseKeyword) return false;
      if (node.kind === ts.SyntaxKind.NullKeyword) return null;
      if (ts.isParenthesizedExpression(node) || ts.isAsExpression(node) || ts.isSatisfiesExpression(node)) return next(node.expression);
      if (ts.isPrefixUnaryExpression(node) && node.operator === ts.SyntaxKind.MinusToken && ts.isNumericLiteral(node.operand)) return -(next(node.operand) as number);
      if (ts.isArrayLiteralExpression(node)) return node.elements.map(next);
      if (ts.isObjectLiteralExpression(node)) {
        const result: { [key: string]: Data } = Object.create(null);
        for (const property of node.properties) {
          if ((!ts.isPropertyAssignment(property) && !ts.isShorthandPropertyAssignment(property)) || ts.isComputedPropertyName(property.name)) {
            fail(property, "only literal properties and const shorthand are supported");
          }
          const key = property.name.text;
          if (Object.hasOwn(result, key)) fail(property, `duplicate property '${key}'`);
          result[key] = next(ts.isPropertyAssignment(property) ? property.initializer : property.name);
        }
        return result;
      }
      if (ts.isIdentifier(node)) {
        const binding = source.bindings.get(node.text);
        if (!binding) fail(node, `unresolved declaration '${node.text}'`);
        if ("expression" in binding) return next(binding.expression);
        const target = imported(binding, source, node);
        return value(target.expression, target.source, depth + 1);
      }
      if (ts.isCallExpression(node) && ts.isIdentifier(node.expression)) {
        const binding = source.bindings.get(node.expression.text);
        if (!binding || !("imported" in binding) || binding.imported !== SDK) fail(node, "unsupported declaration call; do not execute application code to discover metadata");
        if (binding.exported === "defineHost" && node.arguments.length === 1) return next(node.arguments[0]);
        if (binding.exported === "pluginBundle" && node.arguments.length >= 1 && node.arguments.length <= 2) {
          const bundle = next(node.arguments[0]);
          if (typeof bundle !== "string" || !bundle) fail(node, "pluginBundle requires a literal bundle path");
          const options = node.arguments[1] ? next(node.arguments[1]) : {};
          if (!options || Array.isArray(options) || typeof options !== "object" || Object.keys(options).some(key => key !== "execution")) fail(node, "pluginBundle accepts only an execution option");
          const execution = options.execution ?? "bun";
          if (execution !== "bun" && execution !== "process") fail(node, "first Host authoring profile supports bun or process bundles");
          const position = source.file.getLineAndCharacterOfPosition(node.getStart());
          return { bundle: path.resolve(path.dirname(source.file.fileName), bundle), execution, source: `${source.file.fileName}:${position.line + 1}:${position.character + 1}` };
        }
      }
      fail(node, "unsupported declaration expression; use static declarations and packed Plugin references");
    } finally {
      evaluating.delete(node);
    }
  }

  const source = load(filename);
  const output = source.output;
  if (!output || !ts.isCallExpression(output) || !ts.isIdentifier(output.expression)) throw new Error(`${filename}: expected export default defineHost({...})`);
  const factory = source.bindings.get(output.expression.text);
  if (!factory || !("imported" in factory) || factory.imported !== SDK || factory.exported !== "defineHost") fail(output, `Host must use defineHost from '${SDK}'`);
  return value(output, source);
}

if (require.main === module) {
  try {
    if (process.argv.length !== 3) throw new Error("expected one Host source path");
    process.stdout.write(JSON.stringify(extractHost(process.argv[2])));
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}
