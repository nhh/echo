package dev.nhh.echo.server.network;

import dev.nhh.echo.client.dto.VoicePacket;
import dev.nhh.echo.server.model.Channel;

import java.io.IOException;
import java.io.ObjectInputStream;
import java.io.ObjectOutputStream;
import java.net.Socket;
import java.util.ArrayList;
import java.util.concurrent.atomic.AtomicBoolean;

public class ClientConnection extends Thread {

    private Channel channel; // Server Channel

    private ObjectInputStream in; // Stream from client (receive)
    private ObjectOutputStream out; // Stream to client (send)

    private final ArrayList<VoicePacket> queue = new ArrayList<>();
    private AtomicBoolean isRunning = new AtomicBoolean(false);

    public ClientConnection(Socket socket) {
        try {
            this.out = new ObjectOutputStream(socket.getOutputStream()); //create object streams to/from client
            this.in = new ObjectInputStream(socket.getInputStream());
        } catch (IOException e) {
            e.printStackTrace();
        }
    }

    public void send(VoicePacket packet) {
        this.queue.add(packet);
    }

    @Override
    public void run() {
        if (this.isRunning.get()) {
            return;
        }

        this.isRunning.set(true);

        while(isRunning.get()) {
            try {
                VoicePacket vp = (VoicePacket) in.readObject();

                if(vp.getChannelId() != null) {
                    Thread.sleep(10);
                    continue; // Invalid message
                }

                if (this.channel == null) {
                    Thread.sleep(10);
                    continue;
                }
                vp.setTimestamp(System.nanoTime() / 1000000L);
                vp.setChannelId(this.channel.getChannelId());
                this.channel.broadcast(vp); // Send packet to all clients within channel

            } catch (IOException | ClassNotFoundException e) {
                this.isRunning.set(false);
                return;
            } catch (InterruptedException e) {
                e.printStackTrace();
            }

            try {
                if (!queue.isEmpty()) {
                    VoicePacket packet = queue.get(0); // we got something to send to the client
                    queue.remove(0);
                    if (packet == null || packet.getUserId() == null || !packet.isAlive()) { //is the message too old or of an unknown type?
                        System.out.println("dropping packet from " + packet.getUserId() + " to " + packet.getChannelId());
                        continue;
                    }
                    out.writeObject(packet); //send the message
                } else {
                    Thread.sleep(10); //avoid busy wait
                }
            } catch(IOException | InterruptedException e) {
                e.printStackTrace();
            }

        }

        this.channel.removeClient(this);

    }

    public void setChannel(Channel channel) {
        this.channel = channel;
    }
}
