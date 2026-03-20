# Snapshot/Restore Progress Tracking — TanStack Frontend Integration

This document describes how to consume the clawmacdo snapshot/restore progress tracking API from a **TanStack** (React Query + TanStack Router) frontend.

---

## Architecture Overview

```
┌──────────────────────┐      POST /api/snapshots/restore
│  TanStack Frontend   │ ──────────────────────────────────► ┌─────────────────┐
│                      │ ◄── { ok, operation_id }            │  clawmacdo API  │
│  useMutation()       │                                     │  (Axum/Rust)    │
│  useOperationSSE()   │      GET /api/deploy/{id}/events    │                 │
│  useQuery()          │ ◄── SSE: [Step 2/5] ...             │  tokio::spawn   │
│                      │ ◄── SSE: RESTORE_COMPLETE_JSON:{..} │  ───► cloud API │
└──────────────────────┘                                     └─────────────────┘
```

**Flow:**
1. Frontend sends a POST mutation to start the operation
2. Backend returns `{ ok: true, operation_id: "..." }` immediately
3. Frontend opens an `EventSource` SSE connection to stream progress
4. Backend emits `[Step N/T]` messages parsed by the frontend for progress bars
5. Terminal messages: `SNAPSHOT_COMPLETE_JSON:`, `RESTORE_COMPLETE_JSON:`, `SNAPSHOT_ERROR:`, `RESTORE_ERROR:`

---

## API Contracts

### POST `/api/deployments/{id}/snapshot`

Start a snapshot operation for a deployed instance.

**Request:**
```typescript
interface SnapshotRequest {
  snapshot_name: string
  do_token?: string           // DigitalOcean
  aws_region?: string         // Lightsail
  byteplus_access_key?: string // BytePlus
  byteplus_secret_key?: string
  byteplus_region?: string
}
```

**Response:**
```typescript
interface SnapshotResponse {
  ok: boolean
  message: string
  operation_id?: string // present when ok=true
}
```

### POST `/api/snapshots/restore`

Start a restore operation from a snapshot.

**Request:**
```typescript
interface RestoreRequest {
  snapshot_name: string
  provider: 'digitalocean' | 'lightsail' | 'byteplus'
  do_token?: string
  aws_region?: string
  byteplus_access_key?: string
  byteplus_secret_key?: string
  byteplus_region?: string
  size?: string
  spot?: boolean
}
```

**Response:**
```typescript
interface RestoreResponse {
  ok: boolean
  message: string
  operation_id?: string
}
```

### GET `/api/deploy/{operation_id}/events` (SSE)

Stream real-time progress events. Each SSE `data:` line is one of:

| Pattern | Meaning |
|---------|---------|
| `[Step N/T] Label...` | Progress update (step N of T) |
| `  Detail text...` | Detail line for current step |
| `SNAPSHOT_COMPLETE_JSON:{...}` | Snapshot finished successfully |
| `RESTORE_COMPLETE_JSON:{...}` | Restore finished successfully |
| `SNAPSHOT_ERROR:message` | Snapshot failed |
| `RESTORE_ERROR:message` | Restore failed |

**RESTORE_COMPLETE_JSON payload:**
```typescript
interface RestoreCompletePayload {
  deploy_id: string
  hostname: string
  ip: string
  ssh_key_path: string
}
```

**SNAPSHOT_COMPLETE_JSON payload:**
```typescript
interface SnapshotCompletePayload {
  snapshot_name: string
  hostname: string
}
```

### GET `/api/deploy/steps/{operation_id}`

Poll-based alternative — returns all recorded steps from SQLite.

```typescript
interface StepRow {
  deploy_id: string
  step_number: number
  total_steps: number
  label: string
  status: 'running' | 'completed' | 'failed' | 'skipped'
  started_at: string
  completed_at: string | null
  error_msg: string | null
}
```

---

## TanStack Implementation

### 1. Types (`src/types/operations.ts`)

```typescript
export type OperationType = 'snapshot' | 'restore'
export type OperationStatus = 'idle' | 'pending' | 'running' | 'completed' | 'error'

export interface OperationProgress {
  operationId: string | null
  type: OperationType
  status: OperationStatus
  currentStep: number
  totalSteps: number
  currentLabel: string
  logs: string[]
  error: string | null
  result: Record<string, unknown> | null
}
```

### 2. API client (`src/api/operations.ts`)

```typescript
import { SnapshotRequest, RestoreRequest } from '../types/operations'

const API_BASE = import.meta.env.VITE_API_URL || 'http://localhost:3456'

export async function startSnapshot(
  deploymentId: string,
  data: SnapshotRequest
): Promise<{ ok: boolean; message: string; operation_id?: string }> {
  const res = await fetch(
    `${API_BASE}/api/deployments/${deploymentId}/snapshot`,
    {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(data),
    }
  )
  return res.json()
}

export async function startRestore(
  data: RestoreRequest
): Promise<{ ok: boolean; message: string; operation_id?: string }> {
  const res = await fetch(`${API_BASE}/api/snapshots/restore`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data),
  })
  return res.json()
}

export function createOperationEventSource(operationId: string): EventSource {
  return new EventSource(
    `${API_BASE}/api/deploy/${operationId}/events`
  )
}
```

### 3. SSE hook (`src/hooks/useOperationSSE.ts`)

This is the core hook that connects to the SSE endpoint and parses progress messages.

```typescript
import { useEffect, useRef, useCallback, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { createOperationEventSource } from '../api/operations'
import type { OperationProgress, OperationType } from '../types/operations'

const STEP_REGEX = /\[Step (\d+)\/(\d+)\]\s*(.*)/

export function useOperationSSE(
  operationId: string | null,
  type: OperationType
) {
  const queryClient = useQueryClient()
  const esRef = useRef<EventSource | null>(null)
  const [progress, setProgress] = useState<OperationProgress>({
    operationId: null,
    type,
    status: 'idle',
    currentStep: 0,
    totalSteps: 0,
    currentLabel: '',
    logs: [],
    error: null,
    result: null,
  })

  const cleanup = useCallback(() => {
    if (esRef.current) {
      esRef.current.close()
      esRef.current = null
    }
  }, [])

  useEffect(() => {
    if (!operationId) return

    cleanup()

    setProgress((prev) => ({
      ...prev,
      operationId,
      status: 'running',
      currentStep: 0,
      totalSteps: 0,
      currentLabel: 'Starting...',
      logs: [],
      error: null,
      result: null,
    }))

    const es = createOperationEventSource(operationId)
    esRef.current = es

    es.onmessage = (event) => {
      const msg = event.data

      // Check for terminal messages
      const completePrefix =
        type === 'snapshot'
          ? 'SNAPSHOT_COMPLETE_JSON:'
          : 'RESTORE_COMPLETE_JSON:'
      const errorPrefix =
        type === 'snapshot' ? 'SNAPSHOT_ERROR:' : 'RESTORE_ERROR:'

      if (msg.startsWith(completePrefix)) {
        const payload = JSON.parse(msg.slice(completePrefix.length))
        setProgress((prev) => ({
          ...prev,
          status: 'completed',
          currentStep: prev.totalSteps,
          currentLabel: 'Complete',
          result: payload,
        }))
        // Invalidate deployments query to refresh the list
        queryClient.invalidateQueries({ queryKey: ['deployments'] })
        cleanup()
        return
      }

      if (msg.startsWith(errorPrefix)) {
        const errorMsg = msg.slice(errorPrefix.length)
        setProgress((prev) => ({
          ...prev,
          status: 'error',
          error: errorMsg,
        }))
        cleanup()
        return
      }

      // Parse step progress
      const stepMatch = msg.match(STEP_REGEX)
      if (stepMatch) {
        const [, current, total, label] = stepMatch
        setProgress((prev) => ({
          ...prev,
          currentStep: parseInt(current, 10),
          totalSteps: parseInt(total, 10),
          currentLabel: label,
          logs: [...prev.logs, msg],
        }))
        return
      }

      // Append as log line
      setProgress((prev) => ({
        ...prev,
        logs: [...prev.logs, msg],
      }))
    }

    es.onerror = () => {
      // SSE errors are normal when the stream ends
      cleanup()
    }

    return cleanup
  }, [operationId, type, cleanup, queryClient])

  return progress
}
```

### 4. Mutation hooks (`src/hooks/useSnapshotMutation.ts`)

```typescript
import { useMutation } from '@tanstack/react-query'
import { useState } from 'react'
import { startSnapshot, startRestore } from '../api/operations'
import { useOperationSSE } from './useOperationSSE'

export function useSnapshotMutation(deploymentId: string) {
  const [operationId, setOperationId] = useState<string | null>(null)
  const progress = useOperationSSE(operationId, 'snapshot')

  const mutation = useMutation({
    mutationFn: (data: { snapshot_name: string; [key: string]: unknown }) =>
      startSnapshot(deploymentId, data),
    onSuccess: (res) => {
      if (res.ok && res.operation_id) {
        setOperationId(res.operation_id)
      }
    },
  })

  const reset = () => {
    setOperationId(null)
    mutation.reset()
  }

  return { mutation, progress, reset }
}

export function useRestoreMutation() {
  const [operationId, setOperationId] = useState<string | null>(null)
  const progress = useOperationSSE(operationId, 'restore')

  const mutation = useMutation({
    mutationFn: (data: {
      provider: string
      snapshot_name: string
      [key: string]: unknown
    }) => startRestore(data),
    onSuccess: (res) => {
      if (res.ok && res.operation_id) {
        setOperationId(res.operation_id)
      }
    },
  })

  const reset = () => {
    setOperationId(null)
    mutation.reset()
  }

  return { mutation, progress, reset }
}
```

### 5. Progress bar component (`src/components/OperationProgress.tsx`)

```tsx
import type { OperationProgress } from '../types/operations'

interface Props {
  progress: OperationProgress
  onClose?: () => void
}

export function OperationProgressBar({ progress, onClose }: Props) {
  const { status, currentStep, totalSteps, currentLabel, error, logs } =
    progress
  const percentage =
    totalSteps > 0 ? Math.round((currentStep / totalSteps) * 100) : 0

  if (status === 'idle') return null

  return (
    <div className="rounded-lg border border-slate-700 bg-slate-800 p-4">
      {/* Header */}
      <div className="mb-2 flex items-center justify-between">
        <span className="text-sm font-medium text-slate-200">
          {status === 'completed'
            ? 'Operation Complete'
            : status === 'error'
              ? 'Operation Failed'
              : currentLabel}
        </span>
        {(status === 'completed' || status === 'error') && onClose && (
          <button
            onClick={onClose}
            className="text-xs text-slate-400 hover:text-white"
          >
            Dismiss
          </button>
        )}
      </div>

      {/* Progress bar */}
      <div className="h-2 w-full rounded-full bg-slate-700">
        <div
          className={`h-2 rounded-full transition-all duration-300 ${
            status === 'error'
              ? 'bg-red-500'
              : status === 'completed'
                ? 'bg-green-500'
                : 'bg-blue-500'
          }`}
          style={{ width: `${percentage}%` }}
        />
      </div>

      {/* Step counter */}
      <div className="mt-1 flex justify-between text-xs text-slate-400">
        <span>
          Step {currentStep}/{totalSteps}
        </span>
        <span>{percentage}%</span>
      </div>

      {/* Error message */}
      {error && (
        <div className="mt-2 rounded bg-red-900/30 p-2 text-xs text-red-300">
          {error}
        </div>
      )}

      {/* Expandable log */}
      <details className="mt-2">
        <summary className="cursor-pointer text-xs text-slate-500">
          Show logs ({logs.length} lines)
        </summary>
        <pre className="mt-1 max-h-40 overflow-y-auto rounded bg-slate-900 p-2 text-xs text-slate-300">
          {logs.join('\n')}
        </pre>
      </details>
    </div>
  )
}
```

### 6. Usage in a page component (`src/routes/deployments.$id.tsx`)

Using TanStack Router's route component pattern:

```tsx
import { createFileRoute } from '@tanstack/react-router'
import { useState } from 'react'
import { useSnapshotMutation, useRestoreMutation } from '../hooks/useSnapshotMutation'
import { OperationProgressBar } from '../components/OperationProgress'

export const Route = createFileRoute('/deployments/$id')({
  component: DeploymentDetail,
})

function DeploymentDetail() {
  const { id } = Route.useParams()
  const [snapshotName, setSnapshotName] = useState('')

  // Snapshot with progress
  const {
    mutation: snapshotMutation,
    progress: snapshotProgress,
    reset: resetSnapshot,
  } = useSnapshotMutation(id)

  const handleSnapshot = () => {
    snapshotMutation.mutate({
      snapshot_name: snapshotName,
      do_token: localStorage.getItem('do_token') || '',
    })
  }

  return (
    <div className="space-y-4 p-6">
      <h1 className="text-xl font-bold text-white">Deployment {id}</h1>

      {/* Snapshot form */}
      <div className="flex gap-2">
        <input
          type="text"
          value={snapshotName}
          onChange={(e) => setSnapshotName(e.target.value)}
          placeholder="Snapshot name"
          className="rounded bg-slate-800 px-3 py-2 text-white"
        />
        <button
          onClick={handleSnapshot}
          disabled={
            snapshotMutation.isPending ||
            snapshotProgress.status === 'running'
          }
          className="rounded bg-blue-600 px-4 py-2 text-white disabled:opacity-50"
        >
          {snapshotProgress.status === 'running'
            ? 'Creating...'
            : 'Create Snapshot'}
        </button>
      </div>

      {/* Progress bar */}
      <OperationProgressBar
        progress={snapshotProgress}
        onClose={resetSnapshot}
      />
    </div>
  )
}
```

### 7. Restore page (`src/routes/restore.tsx`)

```tsx
import { createFileRoute } from '@tanstack/react-router'
import { useState } from 'react'
import { useRestoreMutation } from '../hooks/useSnapshotMutation'
import { OperationProgressBar } from '../components/OperationProgress'

export const Route = createFileRoute('/restore')({
  component: RestorePage,
})

function RestorePage() {
  const [form, setForm] = useState({
    provider: 'digitalocean',
    snapshot_name: '',
    do_token: '',
    size: '',
  })

  const {
    mutation: restoreMutation,
    progress: restoreProgress,
    reset: resetRestore,
  } = useRestoreMutation()

  const handleRestore = () => {
    restoreMutation.mutate(form)
  }

  return (
    <div className="space-y-4 p-6">
      <h1 className="text-xl font-bold text-white">
        Restore from Snapshot
      </h1>

      {/* Form fields ... */}
      <button
        onClick={handleRestore}
        disabled={
          restoreMutation.isPending ||
          restoreProgress.status === 'running'
        }
        className="rounded bg-green-600 px-4 py-2 text-white disabled:opacity-50"
      >
        {restoreProgress.status === 'running'
          ? `Restoring... (${restoreProgress.currentStep}/${restoreProgress.totalSteps})`
          : 'Restore'}
      </button>

      <OperationProgressBar
        progress={restoreProgress}
        onClose={resetRestore}
      />

      {/* Show result on completion */}
      {restoreProgress.status === 'completed' &&
        restoreProgress.result && (
          <div className="rounded border border-green-800 bg-green-900/20 p-4">
            <h3 className="font-medium text-green-300">
              Restore Complete
            </h3>
            <dl className="mt-2 space-y-1 text-sm text-slate-300">
              <div>
                <dt className="inline font-medium">Hostname:</dt>{' '}
                <dd className="inline">
                  {(restoreProgress.result as any).hostname}
                </dd>
              </div>
              <div>
                <dt className="inline font-medium">IP:</dt>{' '}
                <dd className="inline">
                  {(restoreProgress.result as any).ip}
                </dd>
              </div>
            </dl>
          </div>
        )}
    </div>
  )
}
```

---

## Poll-Based Alternative (without SSE)

If SSE is not desirable (e.g., behind a proxy that buffers SSE), use TanStack Query polling:

```typescript
import { useQuery } from '@tanstack/react-query'

function useOperationSteps(operationId: string | null) {
  return useQuery({
    queryKey: ['operation-steps', operationId],
    queryFn: async () => {
      const res = await fetch(
        `${API_BASE}/api/deploy/steps/${operationId}`
      )
      return res.json() as Promise<StepRow[]>
    },
    enabled: !!operationId,
    refetchInterval: (query) => {
      // Stop polling when all steps are done
      const steps = query.state.data
      if (!steps || steps.length === 0) return 2000
      const allDone = steps.every(
        (s) =>
          s.status === 'completed' ||
          s.status === 'failed' ||
          s.status === 'skipped'
      )
      return allDone ? false : 2000 // poll every 2s while running
    },
  })
}
```

---

## Step Counts by Operation

| Operation | Provider | Total Steps | Longest Step |
|-----------|----------|-------------|-------------|
| Snapshot | DigitalOcean | 3 (or 5 with power-off) | Creating snapshot (~3-5 min) |
| Snapshot | Lightsail | 3 | Creating snapshot (~3-5 min) |
| Snapshot | BytePlus | 4 | Creating snapshot (~3-5 min) |
| Restore | DigitalOcean | 5 | Waiting for droplet (~1-3 min) |
| Restore | Lightsail | 5 | Waiting for instance (~1-3 min) |
| Restore | BytePlus | 7 | Creating image from snapshot (~3-5 min) |

---

## File Summary

| File | Purpose |
|------|---------|
| `src/types/operations.ts` | TypeScript interfaces for progress state |
| `src/api/operations.ts` | API client functions (fetch + EventSource) |
| `src/hooks/useOperationSSE.ts` | Core SSE hook — parses progress messages into state |
| `src/hooks/useSnapshotMutation.ts` | `useMutation` + SSE wiring for snapshot/restore |
| `src/components/OperationProgress.tsx` | Reusable progress bar component |
| `src/routes/deployments.$id.tsx` | Snapshot from deployment detail page |
| `src/routes/restore.tsx` | Restore page with progress |
