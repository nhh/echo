package dev.nhh.echo.audio.speaker;

import java.util.Queue;
import java.util.concurrent.ConcurrentLinkedQueue;

public enum SpeakerQueue {

    INSTANCE;

    private final ConcurrentLinkedQueue<AudioPacket> queue = new ConcurrentLinkedQueue<>();

    public void add(byte[] bytes, int off, int length) {
        this.queue.add(new AudioPacket(bytes, off, length));
    }

    public Queue<AudioPacket> getQueue() {
        return queue;
    }

}
