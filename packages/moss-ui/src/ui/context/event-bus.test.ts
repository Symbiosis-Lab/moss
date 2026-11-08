import { describe, it, expect, vi, beforeEach } from "vitest";
import { eventBus, Events } from "./event-bus";

describe("EventBus", () => {
  beforeEach(() => {
    // Clear all listeners before each test
    eventBus.clear();
  });

  describe("on/emit", () => {
    it("should call callback when event is emitted", () => {
      const callback = vi.fn();
      eventBus.on("test-event", callback);

      eventBus.emit("test-event", { data: "hello" });

      expect(callback).toHaveBeenCalledTimes(1);
      expect(callback).toHaveBeenCalledWith({ data: "hello" });
    });

    it("should call multiple callbacks for same event", () => {
      const callback1 = vi.fn();
      const callback2 = vi.fn();
      eventBus.on("test-event", callback1);
      eventBus.on("test-event", callback2);

      eventBus.emit("test-event", "data");

      expect(callback1).toHaveBeenCalledTimes(1);
      expect(callback2).toHaveBeenCalledTimes(1);
    });

    it("should not call callbacks for different events", () => {
      const callback = vi.fn();
      eventBus.on("event-a", callback);

      eventBus.emit("event-b", "data");

      expect(callback).not.toHaveBeenCalled();
    });

    it("should emit to correct event when multiple events exist", () => {
      const callbackA = vi.fn();
      const callbackB = vi.fn();
      eventBus.on("event-a", callbackA);
      eventBus.on("event-b", callbackB);

      eventBus.emit("event-a", "data-a");

      expect(callbackA).toHaveBeenCalledWith("data-a");
      expect(callbackB).not.toHaveBeenCalled();
    });
  });

  describe("off", () => {
    it("should remove callback when unsubscribed", () => {
      const callback = vi.fn();
      eventBus.on("test-event", callback);
      eventBus.off("test-event", callback);

      eventBus.emit("test-event", "data");

      expect(callback).not.toHaveBeenCalled();
    });

    it("should only remove specific callback", () => {
      const callback1 = vi.fn();
      const callback2 = vi.fn();
      eventBus.on("test-event", callback1);
      eventBus.on("test-event", callback2);
      eventBus.off("test-event", callback1);

      eventBus.emit("test-event", "data");

      expect(callback1).not.toHaveBeenCalled();
      expect(callback2).toHaveBeenCalledTimes(1);
    });
  });

  describe("unsubscribe function", () => {
    it("should return unsubscribe function from on()", () => {
      const callback = vi.fn();
      const unsubscribe = eventBus.on("test-event", callback);

      // First emit should work
      eventBus.emit("test-event", "data1");
      expect(callback).toHaveBeenCalledTimes(1);

      // Unsubscribe
      unsubscribe();

      // Second emit should not call callback
      eventBus.emit("test-event", "data2");
      expect(callback).toHaveBeenCalledTimes(1);
    });
  });

  describe("clear", () => {
    it("should clear specific event listeners", () => {
      const callback = vi.fn();
      eventBus.on("test-event", callback);

      eventBus.clear("test-event");
      eventBus.emit("test-event", "data");

      expect(callback).not.toHaveBeenCalled();
    });

    it("should clear all listeners when no event specified", () => {
      const callbackA = vi.fn();
      const callbackB = vi.fn();
      eventBus.on("event-a", callbackA);
      eventBus.on("event-b", callbackB);

      eventBus.clear();
      eventBus.emit("event-a", "data");
      eventBus.emit("event-b", "data");

      expect(callbackA).not.toHaveBeenCalled();
      expect(callbackB).not.toHaveBeenCalled();
    });
  });

  describe("listenerCount", () => {
    it("should return 0 for event with no listeners", () => {
      expect(eventBus.listenerCount("non-existent")).toBe(0);
    });

    it("should return correct count for event with listeners", () => {
      eventBus.on("test-event", () => {});
      eventBus.on("test-event", () => {});

      expect(eventBus.listenerCount("test-event")).toBe(2);
    });

    it("should decrease count when listener removed", () => {
      const callback = vi.fn();
      eventBus.on("test-event", callback);
      eventBus.on("test-event", () => {});

      expect(eventBus.listenerCount("test-event")).toBe(2);

      eventBus.off("test-event", callback);
      expect(eventBus.listenerCount("test-event")).toBe(1);
    });
  });

  describe("type safety with Events constants", () => {
    it("should work with Events constants", () => {
      const callback = vi.fn();
      eventBus.on(Events.SYNDICATION_STATE_CHANGED, callback);

      eventBus.emit(Events.SYNDICATION_STATE_CHANGED, {
        enabledPlatforms: ["matters"],
      });

      expect(callback).toHaveBeenCalledWith({
        enabledPlatforms: ["matters"],
      });
    });
  });

  describe("DEPLOY_PROGRESS event", () => {
    it("should emit and receive deploy progress event with isDeploying true", () => {
      const callback = vi.fn();
      eventBus.on(Events.DEPLOY_PROGRESS, callback);

      eventBus.emit(Events.DEPLOY_PROGRESS, {
        isDeploying: true,
        status: "in_progress",
      });

      expect(callback).toHaveBeenCalledTimes(1);
      expect(callback).toHaveBeenCalledWith({
        isDeploying: true,
        status: "in_progress",
      });
    });

    it("should emit and receive deploy progress event with isDeploying false", () => {
      const callback = vi.fn();
      eventBus.on(Events.DEPLOY_PROGRESS, callback);

      eventBus.emit(Events.DEPLOY_PROGRESS, {
        isDeploying: false,
        status: "success",
      });

      expect(callback).toHaveBeenCalledTimes(1);
      expect(callback).toHaveBeenCalledWith({
        isDeploying: false,
        status: "success",
      });
    });

    it("should emit deploy progress event without status", () => {
      const callback = vi.fn();
      eventBus.on(Events.DEPLOY_PROGRESS, callback);

      eventBus.emit(Events.DEPLOY_PROGRESS, {
        isDeploying: false,
      });

      expect(callback).toHaveBeenCalledTimes(1);
      expect(callback).toHaveBeenCalledWith({
        isDeploying: false,
      });
    });

    it("should emit deploy progress event with error status", () => {
      const callback = vi.fn();
      eventBus.on(Events.DEPLOY_PROGRESS, callback);

      eventBus.emit(Events.DEPLOY_PROGRESS, {
        isDeploying: false,
        status: "error",
      });

      expect(callback).toHaveBeenCalledTimes(1);
      expect(callback).toHaveBeenCalledWith({
        isDeploying: false,
        status: "error",
      });
    });
  });
});
