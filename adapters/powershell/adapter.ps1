param()

$ErrorActionPreference = 'Stop'
$MaxMessageBytes = 4 * 1024 * 1024

function Write-ProtocolMessage {
    param([Parameter(Mandatory = $true)] $Value)

    $json = $Value | ConvertTo-Json -Compress -Depth 32
    [Console]::Out.WriteLine($json)
    [Console]::Out.Flush()
}

function New-ErrorResponse {
    param($Id, [int] $Code, [string] $Message)

    [ordered]@{
        jsonrpc = '2.0'
        id = $Id
        error = [ordered]@{
            code = $Code
            message = $Message
        }
    }
}

function New-ResultResponse {
    param($Id, $Result)

    [ordered]@{
        jsonrpc = '2.0'
        id = $Id
        result = $Result
    }
}

function Invoke-Parse {
    param([string] $Source)

    $tokens = $null
    $parseErrors = $null
    $ast = [System.Management.Automation.Language.Parser]::ParseInput(
        $Source,
        [ref] $tokens,
        [ref] $parseErrors
    )

    $diagnostics = @($parseErrors | ForEach-Object {
        [ordered]@{
            message = $_.Message
            error_id = $_.ErrorId
            start_offset = $_.Extent.StartOffset
            end_offset = $_.Extent.EndOffset
            start_line = $_.Extent.StartLineNumber
            start_column = $_.Extent.StartColumnNumber
            end_line = $_.Extent.EndLineNumber
            end_column = $_.Extent.EndColumnNumber
        }
    })
    $encodedTokens = @($tokens | ForEach-Object {
        [ordered]@{
            kind = $_.Kind.ToString()
            text = $_.Text
            start_offset = $_.Extent.StartOffset
            end_offset = $_.Extent.EndOffset
        }
    })

    [ordered]@{
        valid = ($diagnostics.Count -eq 0)
        parser = 'System.Management.Automation.Language.Parser'
        runtime_version = $PSVersionTable.PSVersion.ToString()
        ast_kind = $ast.GetType().FullName
        diagnostics = $diagnostics
        tokens = $encodedTokens
    }
}

while ($true) {
    $line = [Console]::In.ReadLine()
    if ($null -eq $line) {
        break
    }

    if ([System.Text.Encoding]::UTF8.GetByteCount($line) -gt $MaxMessageBytes) {
        $oversizedId = $null
        try {
            $oversizedId = ($line | ConvertFrom-Json).id
        }
        catch {
            # An oversized malformed request has no trustworthy response id.
        }
        Write-ProtocolMessage (New-ErrorResponse $oversizedId -32002 'adapter message exceeds the byte limit')
        continue
    }

    $id = $null
    try {
        $request = $line | ConvertFrom-Json
    }
    catch {
        Write-ProtocolMessage (New-ErrorResponse $null -32700 $_.Exception.Message)
        continue
    }

    try {
        $id = $request.id
        if ($request.jsonrpc -ne '2.0') {
            Write-ProtocolMessage (New-ErrorResponse $id -32600 'invalid JSON-RPC request')
            continue
        }
        if ($request.method -isnot [string]) {
            Write-ProtocolMessage (New-ErrorResponse $id -32600 'request method must be a string')
            continue
        }

        switch ($request.method) {
            'deshell.handshake' {
                if ($request.params.protocol_version -ne 1) {
                    Write-ProtocolMessage (New-ErrorResponse $id -32001 'unsupported protocol version')
                    continue
                }
                Write-ProtocolMessage (New-ResultResponse $id ([ordered]@{
                    protocol_version = 1
                    server = [ordered]@{
                        name = 'deshell-powershell-official-ast'
                        version = '0.1.0'
                    }
                    capabilities = @('frontend.detect', 'frontend.parse')
                }))
            }
            'frontend.detect' {
                Write-ProtocolMessage (New-ResultResponse $id ([ordered]@{
                    interpreter = 'powershell'
                    confidence = 'certain'
                }))
            }
            'frontend.parse' {
                if ($request.params.source -isnot [string]) {
                    Write-ProtocolMessage (New-ErrorResponse $id -32602 'params.source must be a string')
                    continue
                }
                Write-ProtocolMessage (New-ResultResponse $id (Invoke-Parse ([string] $request.params.source)))
            }
            default {
                Write-ProtocolMessage (New-ErrorResponse $id -32601 'method not found')
            }
        }
    }
    catch {
        Write-ProtocolMessage (New-ErrorResponse $id -32603 $_.Exception.Message)
    }
}
