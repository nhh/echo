package dev.nhh.echoclient.audio.speaker;

import java.util.Queue;
import java.util.concurrent.ConcurrentLinkedQueue;

public enum SpeakerQueue {

    INSTANCE;

    private final Queue<AudioPacket> queue = new ConcurrentLinkedQueue<>();

    public void add(byte[] bytes, int off, int length) {
        this.queue.add(new AudioPacket(bytes, off, length));
    }

    public Queue<AudioPacket> getQueue() {
        return queue;
    }

}
