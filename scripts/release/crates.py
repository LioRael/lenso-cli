#!/usr/bin/env python3
"""Publish the CLI's public Cargo dependency graph in topological order."""
import argparse
import json
from pathlib import Path
import subprocess
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
PATHS = ['crates/framework', 'crates/composition', 'crates/codegen', 'crates/runtime', 'crates/agent', 'extensions/project', 'extensions/ops', 'crates/cli']
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--only', choices=PATHS, help='Select one public package directory')
parser.add_argument('--status', action='store_true', help='Write publication status to GitHub Actions outputs')
parser.add_argument('--publish', action='store_true', help='Upload missing versions; normal Cargo package verification remains enabled')
args = parser.parse_args()
metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--no-deps', '--format-version', '1'], cwd=ROOT))
by_manifest = {Path(package['manifest_path']).resolve(): package for package in metadata['packages']}
packages = {by_manifest[(ROOT / directory / 'Cargo.toml').resolve()]['name']: by_manifest[(ROOT / directory / 'Cargo.toml').resolve()] for directory in PATHS}
ordered = []
active = set()
seen = set()
def visit(name):
    if name in seen:
        return
    if name in active:
        raise RuntimeError('Cargo publication dependency cycle')
    active.add(name)
    for dependency in packages[name]['dependencies']:
        if dependency['name'] in packages:
            visit(dependency['name'])
    active.remove(name)
    seen.add(name)
    ordered.append(packages[name])
for name in packages:
    visit(name)

def version_exists(package):
    request = urllib.request.Request('https://crates.io/api/v1/crates/' + package['name'], headers={'User-Agent': 'lenso-cli-release (https://github.com/LioRael/lenso-cli)'})
    for attempt in range(4):
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                data = json.load(response)
            legacy_migration = package['name'] in {'lenso-cli', 'lenso'} and package['version'].startswith('1.') and data['crate'].get('repository') == 'https://github.com/LioRael/lenso'
            if data['crate'].get('repository') != 'https://github.com/LioRael/lenso-cli' and not legacy_migration:
                raise RuntimeError(f"{package['name']} belongs to a different repository; refusing publication")
            return any(version['num'] == package['version'] for version in data['versions'])
        except urllib.error.HTTPError as error:
            if error.code == 404:
                return False
            if error.code < 500 and error.code != 429:
                raise
        except (urllib.error.URLError, TimeoutError):
            pass
        if attempt == 3:
            raise RuntimeError('crates.io status could not be verified')
        time.sleep(2 * (attempt + 1))

if args.only:
    ordered = [package for package in ordered if Path(package['manifest_path']).resolve() == (ROOT / args.only / 'Cargo.toml').resolve()]
for package in ordered:
    print(f"{package['name']} {package['version']}", flush=True)
    if args.status:
        import os
        published = version_exists(package)
        print('Published: ' + str(published).lower(), flush=True)
        if os.environ.get('GITHUB_OUTPUT'):
            with open(os.environ['GITHUB_OUTPUT'], 'a') as output:
                output.write('published=' + str(published).lower() + '\n')
        continue
    if not args.publish:
        continue
    if version_exists(package):
        print('Already published from this repository; leaving immutable version unchanged', flush=True)
        continue
    subprocess.run(['cargo', 'publish', '--package', package['name'], '--locked'], cwd=ROOT, check=True)
    # cargo publish already waits for index propagation before returning.
