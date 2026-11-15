import { invoke as tauriInvoke } from '@tauri-apps/api/core';
import { listen as tauriListen, UnlistenFn } from '@tauri-apps/api/event';
import {
  API_VERSION,
  ConversionRequest,
  DesktopApi,
  DestinationSelection,
  FormatDescriptor,
  InputInspection,
  JobEvent,
  JobSnapshot,
} from './contracts';
import {
  normalizeError,
  validateConversionRequest,
  validateDestinationSelection,
  validateFormatDescriptor,
  validateInputInspection,
  validateJobEvent,
  validateJobSnapshot,
} from './validation';

export const JOB_CHANGED_EVENT = 'loconv://job-changed';

export type InvokeFn = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
export type ListenFn = <T>(
  event: string,
  handler: (event: { payload: T }) => void,
) => Promise<UnlistenFn>;

export class TauriDesktopApi implements DesktopApi {
  private readonly invokeFn: InvokeFn;
  private readonly listenFn: ListenFn;

  constructor(invokeFn?: InvokeFn, listenFn?: ListenFn) {
    this.invokeFn = invokeFn ?? (tauriInvoke as InvokeFn);
    this.listenFn = listenFn ?? (tauriListen as ListenFn);
  }

  async getApiVersion(): Promise<number> {
    try {
      const res = await this.invokeFn('get_api_version', {});
      if (typeof res !== 'number' || !Number.isInteger(res)) {
        throw new Error('API version must be an integer');
      }
      return res;
    } catch (err) {
      throw normalizeError(err);
    }
  }

  async selectInputs(): Promise<InputInspection[]> {
    try {
      const res = await this.invokeFn('select_inputs', {});
      if (!Array.isArray(res)) {
        throw new Error('select_inputs must return an array');
      }
      return res.map(validateInputInspection);
    } catch (err) {
      throw normalizeError(err);
    }
  }

  async importDroppedInputs(paths: string[]): Promise<InputInspection[]> {
    try {
      if (!Array.isArray(paths) || !paths.every((p) => typeof p === 'string' && p.length > 0)) {
        throw {
          code: 'invalid_request',
          message: 'Paths must be an array of non-empty strings.',
          retryable: false,
        };
      }
      const res = await this.invokeFn('import_dropped_inputs', { paths });
      if (!Array.isArray(res)) {
        throw new Error('import_dropped_inputs must return an array');
      }
      return res.map(validateInputInspection);
    } catch (err) {
      throw normalizeError(err);
    }
  }

  async selectDestination(): Promise<DestinationSelection | null> {
    try {
      const res = await this.invokeFn('select_destination', {});
      return validateDestinationSelection(res);
    } catch (err) {
      throw normalizeError(err);
    }
  }

  async listFormats(): Promise<FormatDescriptor[]> {
    try {
      const res = await this.invokeFn('list_formats', {});
      if (!Array.isArray(res)) {
        throw new Error('list_formats must return an array');
      }
      return res.map(validateFormatDescriptor);
    } catch (err) {
      throw normalizeError(err);
    }
  }

  async enqueueJob(request: ConversionRequest): Promise<JobSnapshot> {
    try {
      const validReq = validateConversionRequest(request);
      const res = await this.invokeFn('enqueue_job', { request: validReq });
      return validateJobSnapshot(res);
    } catch (err) {
      throw normalizeError(err);
    }
  }

  async listJobs(): Promise<JobSnapshot[]> {
    try {
      const res = await this.invokeFn('list_jobs', {});
      if (!Array.isArray(res)) {
        throw new Error('list_jobs must return an array');
      }
      return res.map(validateJobSnapshot);
    } catch (err) {
      throw normalizeError(err);
    }
  }

  async cancelJob(jobId: string): Promise<JobSnapshot> {
    try {
      if (typeof jobId !== 'string' || jobId.length === 0) {
        throw {
          code: 'invalid_request',
          message: 'Job ID must be a non-empty string.',
          retryable: false,
        };
      }
      const res = await this.invokeFn('cancel_job', { jobId });
      return validateJobSnapshot(res);
    } catch (err) {
      throw normalizeError(err);
    }
  }

  async onJobChanged(listener: (event: JobEvent) => void): Promise<() => void> {
    try {
      const unlisten = await this.listenFn<unknown>(JOB_CHANGED_EVENT, (rawEvent) => {
        try {
          const validEvent = validateJobEvent(rawEvent.payload);
          if (validEvent.apiVersion !== API_VERSION) {
            // Mismatched apiVersion is rejected/ignored
            return;
          }
          listener(validEvent);
        } catch {
          // Ignore malformed event payloads safely
        }
      });
      return () => {
        unlisten();
      };
    } catch (err) {
      throw normalizeError(err);
    }
  }
}
