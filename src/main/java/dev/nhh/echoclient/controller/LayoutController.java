package dev.nhh.echoclient.controller;

import dev.nhh.echoclient.audio.microphone.Microphone;
import dev.nhh.echoclient.audio.speaker.Speaker;
import dev.nhh.echoclient.util.ThreadScheduler;
import javafx.fxml.FXML;
import javafx.scene.control.TextField;
import javafx.scene.input.MouseEvent;
import javafx.stage.Window;

public class LayoutController {

    @FXML
    private TextField textField;

    @FXML
    private Window dialog;

    @FXML
    protected void startMicrophone(MouseEvent event) {
        var mic = new Thread(new Microphone());
        mic.start();
        ThreadScheduler.INSTANCE.addThread(mic);
    }

    @FXML
    protected void start(MouseEvent event) {
        var mic = new Thread(new Speaker());
        mic.start();
        ThreadScheduler.INSTANCE.addThread(mic);
    }

}
