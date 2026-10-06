export interface CompositionKeyboardEvent {
  key: string;
  shiftKey: boolean;
  isComposing: boolean;
  keyCode?: number;
}

export function createCompositionGuard(now: () => number = Date.now) {
  let composing = false;
  let endedAt = Number.NEGATIVE_INFINITY;
  return {
    start() { composing = true; },
    end() { composing = false; endedAt = now(); },
    shouldIgnore(event: CompositionKeyboardEvent) {
      if (event.keyCode === 229 || event.isComposing || composing) return true;
      if (event.key !== 'Enter' || now() - endedAt >= 100) return false;
      endedAt = Number.NEGATIVE_INFINITY;
      return true;
    },
  };
}

export function shouldSendOnEnter(event: CompositionKeyboardEvent, composition: ReturnType<typeof createCompositionGuard>): boolean {
  return event.key === 'Enter' && !event.shiftKey && !composition.shouldIgnore(event);
}
