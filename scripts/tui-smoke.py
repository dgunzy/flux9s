#!/usr/bin/env python3
"""Exercise the real binary in an isolated tmux server against a dev context.

No cluster writes; config, kubeconfig and terminal state are isolated. On failure
print the final screen for CI diagnostics. Requires tmux, kubectl and a built bin.
"""
import argparse
import json
import re
from pathlib import Path
import shlex
import subprocess
import tempfile
import time


def run(*args):
    return subprocess.check_output(args, text=True, timeout=20)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--context', default='kind-flux9s-simple')
    parser.add_argument('--binary', default='target/debug/flux9s')
    parser.add_argument('--metrics-source', choices=['none', 'kubelet'], default='none')
    args = parser.parse_args()
    binary = str(Path(args.binary).resolve())
    # Validate the target and select a running, single-container pod for logs.
    pods = json.loads(run('kubectl', '--context', args.context, '--request-timeout=10s', 'get', 'pods', '-n', 'kube-system', '-o', 'json'))['items']
    pod = next((p for p in pods if p.get('status', {}).get('phase') == 'Running' and len(p['spec']['containers']) == 1), None)
    if pod is None:
        raise RuntimeError(f'{args.context}: no running single-container pod in kube-system for log validation')
    name = pod['metadata']['name']
    node = pod['spec']['nodeName']
    with tempfile.TemporaryDirectory(prefix='flux9s-tui-') as work:
        root = Path(work)
        config = root / 'config'
        config.mkdir()
        (config / 'config.yaml').write_text(f'nativeResources: true\nreadOnly: true\nmetricsSource: {args.metrics_source}\n')
        kubeconfig = root / 'kubeconfig'
        kubeconfig.write_text(run('kubectl', '--context', args.context, 'config', 'view', '--minify', '--flatten', '--raw'))
        kubeconfig.chmod(0o600)
        socket = str(root / 'tmux.sock')

        def tmux(*argv):
            return run('tmux', '-f', '/dev/null', '-S', socket, *argv)

        def screen():
            return tmux('capture-pane', '-p', '-t', 'smoke:0.0')

        def wait_for(*texts):
            deadline = time.monotonic() + 60
            while time.monotonic() < deadline:
                text = screen()
                if all(word in text for word in texts):
                    return
                time.sleep(.2)
            raise AssertionError(f'timed out waiting for {texts}\n{screen()}')

        def command(text):
            tmux('send-keys', '-t', 'smoke:0.0', '-l', ':' + text)
            tmux('send-keys', '-t', 'smoke:0.0', 'Enter')

        env = {'FLUX9S_CONFIG_DIR': str(config), 'FLUX9S_DATA_DIR': str(root / 'data'), 'KUBECONFIG': str(kubeconfig), 'TERM': 'xterm-256color'}
        launch = 'exec env ' + ' '.join(shlex.quote(k + '=' + v) for k, v in env.items()) + ' ' + shlex.quote(binary) + ' -c pods'
        try:
            tmux('new-session', '-d', '-s', 'smoke', '-x', '220', '-y', '55', launch)
            wait_for('RESTARTS', 'NODE', 'CPU')
            command('ns all')
            wait_for('all namespaces')
            tmux('send-keys', '-t', 'smoke:0.0', '-l', '/' + name)
            tmux('send-keys', '-t', 'smoke:0.0', 'Enter')
            wait_for(name)
            if args.metrics_source != 'none':
                deadline = time.monotonic() + 60
                while not any(name in line and len(re.findall(r'use [0-9]', line)) >= 2 for line in screen().splitlines()):
                    if time.monotonic() >= deadline:
                        raise AssertionError('No live CPU and memory usage for the selected pod')
                    time.sleep(.2)
            command('ns kube-system')
            wait_for('ns: kube-system', name)
            tmux('send-keys', '-t', 'smoke:0.0', '-l', '/' + name)
            tmux('send-keys', '-t', 'smoke:0.0', 'Enter')
            wait_for(name, 'req ')
            tmux('send-keys', '-t', 'smoke:0.0', 'Enter')
            wait_for('Describe - Pod', name)
            tmux('send-keys', '-t', 'smoke:0.0', 'Escape')
            wait_for('RESTARTS', name)
            tmux('send-keys', '-t', 'smoke:0.0', '-l', '/' + name)
            tmux('send-keys', '-t', 'smoke:0.0', 'Enter')
            wait_for(name)
            tmux('send-keys', '-t', 'smoke:0.0', 'l')
            wait_for('Logs:', name)
            tmux('send-keys', '-t', 'smoke:0.0', 'Escape')
            command('nodes')
            wait_for('ROLES', 'VERSION', node)
            tmux('send-keys', '-t', 'smoke:0.0', '-l', '/' + node)
            tmux('send-keys', '-t', 'smoke:0.0', 'Enter')
            tmux('send-keys', '-t', 'smoke:0.0', 'Enter')
            wait_for('RESTARTS', ' on ' + node, name, 'all namespaces')
            tmux('send-keys', '-t', 'smoke:0.0', 'Escape')
            wait_for('ROLES', 'VERSION')
            command('ks,nodes')
            wait_for('TYPE', node)
            tmux('send-keys', '-t', 'smoke:0.0', '-l', '/' + node)
            tmux('send-keys', '-t', 'smoke:0.0', 'Enter')
            tmux('send-keys', '-t', 'smoke:0.0', 'Enter')
            wait_for('RESTARTS', ' on ' + node, 'all namespaces')
            tmux('send-keys', '-t', 'smoke:0.0', 'Escape')
            wait_for('TYPE')
            # A small terminal must still accept navigation and quit.
            tmux('resize-window', '-t', 'smoke:0', '-x', '50', '-y', '12')
            command('pods')
            time.sleep(.5)
            tmux('send-keys', '-t', 'smoke:0.0', 'Q')
            deadline = time.monotonic() + 10
            while subprocess.run(['tmux', '-S', socket, 'has-session', '-t', 'smoke'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0:
                if time.monotonic() >= deadline:
                    raise AssertionError('TUI did not quit')
                time.sleep(.1)
            print(f'TUI smoke passed: {args.context} ({args.metrics_source})')
        except Exception:
            try:
                print(screen())
            except subprocess.CalledProcessError:
                pass
            raise
        finally:
            subprocess.run(['tmux', '-S', socket, 'kill-server'], check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


if __name__ == '__main__':
    main()
