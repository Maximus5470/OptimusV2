#!/usr/bin/env pwsh
<#
.SYNOPSIS
    Real-time monitoring script for KEDA autoscaling
.DESCRIPTION
    Displays live updates of pod counts, queue lengths, and HPA metrics
#>

param(
    [int]$RefreshInterval = 3,
    [switch]$ContinuousMode
)

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "OptimusV2 Autoscaling Monitor" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "Press Ctrl+C to stop" -ForegroundColor Yellow
Write-Host ""

function Get-PodCounts {
    $pods = kubectl get pods -n optimus --no-headers 2>$null
    
    $counts = @{
        api = 0
        python = 0
        java = 0
        rust = 0
        redis = 0
        total = 0
    }
    
    foreach ($line in $pods) {
        if ($line -match 'optimus-api') { $counts.api++ }
        elseif ($line -match 'optimus-worker-python') { $counts.python++ }
        elseif ($line -match 'optimus-worker-java') { $counts.java++ }
        elseif ($line -match 'optimus-worker-rust') { $counts.rust++ }
        elseif ($line -match 'redis') { $counts.redis++ }
        $counts.total++
    }
    
    return $counts
}

function Get-QueueLengths {
    $lengths = @{
        python = "N/A"
        java = "N/A"
        rust = "N/A"
    }
    
    try {
        # Get Redis pod name
        $redisPod = kubectl get pods -n optimus -l app=redis --no-headers 2>$null | ForEach-Object { $_.Split()[0] }
        
        if ($redisPod) {
            $pythonLen = kubectl exec -n optimus $redisPod -- redis-cli LLEN "optimus:queue:python" 2>$null
            $javaLen = kubectl exec -n optimus $redisPod -- redis-cli LLEN "optimus:queue:java" 2>$null
            $rustLen = kubectl exec -n optimus $redisPod -- redis-cli LLEN "optimus:queue:rust" 2>$null
            
            if ($pythonLen) { $lengths.python = $pythonLen.Trim() }
            if ($javaLen) { $lengths.java = $javaLen.Trim() }
            if ($rustLen) { $lengths.rust = $rustLen.Trim() }
        }
    }
    catch {
        # Silently continue on errors
    }
    
    return $lengths
}

function Get-HPAStatus {
    $hpaLines = kubectl get hpa -n optimus --no-headers 2>$null
    $hpaData = @()
    
    foreach ($line in $hpaLines) {
        if ($line -match '(\S+)\s+\S+\s+(\d+)/\d+\s+(\d+)\s+(\d+)\s+(\d+)') {
            $hpaData += @{
                name = $matches[1]
                current = $matches[2]
                min = $matches[3]
                max = $matches[4]
                replicas = $matches[5]
            }
        }
    }
    
    return $hpaData
}

do {
    Clear-Host
    
    Write-Host "========================================" -ForegroundColor Cyan
    Write-Host "OptimusV2 Autoscaling Monitor" -ForegroundColor Cyan
    Write-Host "Time: $(Get-Date -Format 'HH:mm:ss')" -ForegroundColor Cyan
    Write-Host "========================================" -ForegroundColor Cyan
    Write-Host ""
    
    # Pod counts
    $pods = Get-PodCounts
    Write-Host "POD COUNTS:" -ForegroundColor Yellow
    Write-Host "  API:           $($pods.api)" -ForegroundColor White
    Write-Host "  Python Worker: $($pods.python)" -ForegroundColor White
    Write-Host "  Java Worker:   $($pods.java)" -ForegroundColor White
    Write-Host "  Rust Worker:   $($pods.rust)" -ForegroundColor White
    Write-Host "  Redis:         $($pods.redis)" -ForegroundColor White
    Write-Host "  Total:         $($pods.total)" -ForegroundColor Green
    Write-Host ""
    
    # Queue lengths
    $queues = Get-QueueLengths
    Write-Host "QUEUE LENGTHS:" -ForegroundColor Yellow
    Write-Host "  Python: $($queues.python)" -ForegroundColor White
    Write-Host "  Java:   $($queues.java)" -ForegroundColor White
    Write-Host "  Rust:   $($queues.rust)" -ForegroundColor White
    Write-Host ""
    
    # HPA Status
    Write-Host "KEDA SCALEDOBJECTS:" -ForegroundColor Yellow
    $scaledObjects = kubectl get scaledobject -n optimus --no-headers 2>$null
    if ($scaledObjects) {
        foreach ($line in $scaledObjects) {
            $parts = $line -split '\s+', 5
            if ($parts.Count -ge 4) {
                $name = $parts[0]
                $ready = $parts[2]
                $active = $parts[3]
                
                $color = if ($active -eq "True") { "Green" } else { "Gray" }
                Write-Host "  $name" -ForegroundColor $color -NoNewline
                Write-Host " (Ready: $ready, Active: $active)" -ForegroundColor White
            }
        }
    }
    else {
        Write-Host "  No ScaledObjects found" -ForegroundColor Gray
    }
    Write-Host ""
    
    Write-Host "Refreshing in ${RefreshInterval}s... (Ctrl+C to stop)" -ForegroundColor DarkGray
    
    Start-Sleep -Seconds $RefreshInterval
    
} while ($ContinuousMode -or $true)
