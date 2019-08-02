package dev.nhh.echo.server.network;

import dev.nhh.echo.server.util.ClientList;
import dev.nhh.echo.client.dto.VoicePacket;

import java.io.IOException;
import java.io.ObjectInputStream;
import java.io.ObjectOutputStream;
import java.net.Socket;
import java.util.ArrayList;
import java.util.UUID;
import java.util.concurrent.atomic.AtomicBoolean;

public class ClientConnection extends Thread {

    private final UUID id;
    private final Socket socket; // Connection to Client

    private ObjectInputStream in;
    private ObjectOutputStream out;

    private final ArrayList<Object> queue = new ArrayList<>();
    private AtomicBoolean isRunning = new AtomicBoolean(false);

    public ClientConnection(UUID id, Socket socket) {
        this.id = id;
        this.socket = socket;
    }

    public void addToQueue(Object e) {
        this.queue.add(e);
    }

    @Override
    public void run() {
        if (this.isRunning.get()) {
            return;
        }

        try {
            this.out = new ObjectOutputStream(socket.getOutputStream()); //create object streams to/from client
            this.in = new ObjectInputStream(socket.getInputStream());
        } catch (IOException ex) {
            ex.printStackTrace();
            return;
        }

        this.isRunning.set(true);

        while(isRunning.get()) {
            try {
                if (in.available() == 0) {
                    Thread.sleep(10);
                }
            } catch (IOException | InterruptedException e) {
                e.printStackTrace();
            }

            try {
                VoicePacket vp = (VoicePacket) in.readObject();

                if(vp.getChannelId() != null) {
                    System.out.println("Invalid message");
                    continue; // Invalid message
                }

                vp.setChannelId(this.id);
                vp.setTimestamp(System.nanoTime() / 1000000L);
                vp.setUserId(this.id);

                ClientList.INSTANCE.broadcast(vp);
            } catch (IOException | ClassNotFoundException e) {
                this.isRunning.set(false);
                System.out.println("Class is not available");
                e.printStackTrace();
            }
        }
    }


}
