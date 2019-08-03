package dev.nhh.echo.server.model;

import dev.nhh.echo.client.dto.VoicePacket;
import dev.nhh.echo.server.network.ClientConnection;

import java.util.ArrayList;
import java.util.UUID;
import java.util.concurrent.atomic.AtomicBoolean;

public class Channel extends Thread {

    private final UUID channelId;
    private final ArrayList<ClientConnection> clients = new ArrayList<>();
    private final ArrayList<VoicePacket> packets = new ArrayList<>();
    private final AtomicBoolean isRunning = new AtomicBoolean(true);

    public Channel(UUID id) {
        this.channelId = id;
    }

    public UUID getChannelId() {
        return this.channelId;
    }

    public void broadcast(VoicePacket packet) {
        this.packets.add(packet);
    }

    public void addClient(ClientConnection client) {
        this.clients.remove(client);
        this.clients.add(client);
        client.setChannel(this);
    }

    public void removeClient(ClientConnection client) {
        this.clients.remove(client);
        client.setChannel(null);
    }

    @Override
    public void run() {

        while(this.isRunning.get()) {

            if(this.packets.isEmpty()) {
                try {
                    Thread.sleep(10);
                } catch (InterruptedException e) {
                    e.printStackTrace();
                }
                continue;
            }

            var packet = this.packets.get(0);
            this.packets.remove(0);

            this.clients.forEach(c -> c.send(packet));
        }

    }

}
