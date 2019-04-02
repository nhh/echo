package dev.nhh.echoclient.util;

import java.util.concurrent.atomic.AtomicBoolean;

public abstract class ThreadWrapper {

    private final Thread thread;
    private final String name;
    protected final AtomicBoolean isRunning = new AtomicBoolean(false);

    public ThreadWrapper(String name, boolean daemon) {
        this.name = name;
        thread = new Thread(this::run);
        thread.setDaemon(daemon);
        this.thread.setName(name);
        this.thread.start();
    }

    public void start() {
        if(isRunning.get()) return;
        this.isRunning.set(true);
    }

    public void stop() {
        if(!isRunning.get()) return;
        this.isRunning.set(false);
        this.thread.interrupt();
    }

    public void run() {
        throw new IllegalStateException("Not yet implemented");
    }

    protected String getName() {
        return name;
    }

}
