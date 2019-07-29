package dev.nhh.echoclient.controller;

import dev.nhh.echoclient.audio.microphone.Microphone;
import dev.nhh.echoclient.audio.speaker.Speaker;
import dev.nhh.echoclient.network.Connection;
import javafx.fxml.FXML;
import javafx.scene.control.TextField;
import javafx.scene.input.MouseEvent;

public class LayoutController {

    @FXML
    private TextField textField;

    protected void stop(MouseEvent event) {
        Microphone.INSTANCE.stop();
        Speaker.INSTANCE.stop();
    }

    @FXML
    protected void start(MouseEvent event) {
        new Thread(new Connection(textField.getText() + "/join")).start();
    }

}
