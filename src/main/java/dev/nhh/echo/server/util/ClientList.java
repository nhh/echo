package dev.nhh.echo.server.util;

import dev.nhh.echo.server.network.ClientConnection;
import dev.nhh.echo.client.dto.VoicePacket;

import java.util.UUID;
import java.util.concurrent.CopyOnWriteArrayList;
import java.util.concurrent.atomic.AtomicBoolean;

public enum ClientList {
    INSTANCE;

    private CopyOnWriteArrayList<ClientConnection> clients = new CopyOnWriteArrayList<>();
    private CopyOnWriteArrayList<Object> broadCastQueue = new CopyOnWriteArrayList<>();
    private Thread broadCastThread;
    private AtomicBoolean isRunning = new AtomicBoolean(false);

    ClientList() {
        this.broadCastThread = new Thread(() -> {
            // Todo sent Message to all clients
            while(this.isRunning.get()) {
                if (broadCastQueue.size() == 0) {
                    try {
                        Thread.sleep(10);
                    } catch (InterruptedException e) {
                        e.printStackTrace();
                    }
                }

                var packet = broadCastQueue.get(0);
                broadCastQueue.remove(0);

                // Todo remove the current user, for no playback
                this.clients.forEach(client -> client.addToQueue(packet));

            }
        });
        this.broadCastThread.start();
    }

    public void add(ClientConnection connection) {
        this.clients.add(connection);
    }

    public ClientConnection getOneConnection(UUID id) {
        return null;
    }

    public void broadcast(VoicePacket packet) {
        System.out.println("Broadcasting");
        this.broadCastQueue.add(packet);
    }

}
