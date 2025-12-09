/// <reference path="./lib/fresh.d.ts" />

/**
 * C# Language Server Support Plugin
 *
 * Handles LSP server requests from C# language servers like:
 * - csharp-language-server (Roslyn-based)
 * - OmniSharp
 *
 * Features:
 * - Auto-restore NuGet packages when the server requests it
 */

interface LspServerRequestData {
  language: string;
  method: string;
  server_command: string;
  params: string | null;
}

interface ProjectNeedsRestoreParams {
  projectFilePath: string;
}

/**
 * Run dotnet restore for a project
 */
async function restoreProject(projectPath: string): Promise<void> {
  editor.setStatus(`Restoring NuGet packages for ${projectPath}...`);
  editor.debug(`csharp_support: Running dotnet restore for ${projectPath}`);

  try {
    const result = await editor.spawnProcess("dotnet", ["restore", projectPath]);

    if (result.exit_code === 0) {
      editor.setStatus(`NuGet restore completed for ${projectPath}`);
      editor.debug(`csharp_support: dotnet restore succeeded`);
    } else {
      editor.setStatus(`NuGet restore failed: ${result.stderr}`);
      editor.debug(`csharp_support: dotnet restore failed: ${result.stderr}`);
    }
  } catch (e) {
    editor.setStatus(`NuGet restore error: ${e}`);
    editor.debug(`csharp_support: dotnet restore error: ${e}`);
  }
}

/**
 * Handle LSP server requests from C# language servers (Roslyn-based)
 */
globalThis.on_csharp_lsp_server_request = function (
  data: LspServerRequestData
): void {
  // Only handle requests from C# language servers
  if (data.server_command !== "csharp-ls" && data.server_command !== "csharp-language-server") {
    return;
  }

  editor.debug(
    `csharp_support: Received LSP request ${data.method} from ${data.server_command}`
  );

  switch (data.method) {
    case "workspace/_roslyn_projectNeedsRestore": {
      // Roslyn LSP server is asking us to restore a project
      if (data.params) {
        try {
          const params: ProjectNeedsRestoreParams = JSON.parse(data.params);
          if (params.projectFilePath) {
            restoreProject(params.projectFilePath);
          }
        } catch (e) {
          editor.debug(`csharp_support: Failed to parse params: ${e}`);
        }
      }
      break;
    }

    default:
      // Log unhandled requests for debugging
      editor.debug(
        `csharp_support: Unhandled LSP request: ${data.method}`
      );
  }
};

// Register hook for LSP server requests
editor.on("lsp_server_request", "on_csharp_lsp_server_request");
