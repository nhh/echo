package dev.nhh.echoclient.util;

import java.util.concurrent.ConcurrentHashMap;

public enum ThreadScheduler {

    INSTANCE;

    private final ConcurrentHashMap<String, ThreadWrapper> threads = new ConcurrentHashMap<>();

    public void stopAll() {
        this.threads
                .values()
                .forEach(ThreadWrapper::stop);
    }

    public void startAll() {
        this.threads
                .values()
                .forEach(ThreadWrapper::start);
    }

    public void addWrapper(ThreadWrapper wrapper) {
        this.threads.putIfAbsent(wrapper.getName(), wrapper);
    }

    public void start(String threadName) {
        final var wrapper = this.threads.get(threadName);
        if(wrapper == null) return;
        wrapper.start();
    }

    public void stop(String threadName) {
        final var wrapper = this.threads.get(threadName);
        if(wrapper == null) return;
        wrapper.stop();
    }

}
