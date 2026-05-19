// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2

import { ExtensionContext, Uri, window, } from 'vscode';
import { LanguageClient, LanguageClientOptions, ServerOptions} from 'vscode-languageclient/node';

let client: LanguageClient;

export async function activate(context: ExtensionContext) {
  const channel = window.createOutputChannel('t32 Language Server', { log: true });

  const command = getLanguageServerPath(context, client);
  const serverOptions: ServerOptions = {
    command: command,
    args: [`--clientProcessId=${process.pid}`],
  };

  let traceChannel = channel;
  if (process.env.NODE_ENV! === 'development') {
    traceChannel = window.createOutputChannel('t32 Language Server Trace', { log: true });
  }

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ language: 'practice', scheme: 'file' }],
    outputChannel: channel,
    traceOutputChannel: traceChannel,
  };

  client = new LanguageClient(
    't32ls',
    't32-language-server',
    serverOptions,
    clientOptions
  );

  try {
    await client.start();
  } catch (error) {
    client.error('Cannot start language server: ', error, 'force');
    throw new Error;
  }
  channel.appendLine('Server has started.');
}

export function deactivate() {
  return client!.stop();
}

function getLanguageServerPath(context: ExtensionContext, client: LanguageClient): string {
  let suffix: string = '';
  if (process.platform === 'win32') {
    suffix = '.exe';
  }
  else if (process.platform === 'linux' || process.platform === 'darwin') {
    suffix = '';
  } else {
    client.error('Operating system is not supported.', 'force');
    throw new Error;
  }

  if (process.env.NODE_ENV! === 'development') {
      return Uri.joinPath(context.extensionUri, '..', 'target', 'debug', 't32ls' + suffix).fsPath;
  }
  return Uri.joinPath(context.extensionUri, 'bin', 't32ls' + suffix).fsPath;
}
