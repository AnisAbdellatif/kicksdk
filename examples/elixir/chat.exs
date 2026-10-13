# Prints a Kick channel's chat:  elixir chat.exs xqc
#
# kicksdk builds the requests and speaks the protocol; Req sends the requests
# and WebSockex holds the socket. Builds the NIF from source (needs Rust).

Mix.install([
  {:kicksdk, path: Path.expand("../../bindings/elixir", __DIR__)},
  {:rustler, "~> 0.38"},
  {:req, "~> 0.5"},
  {:websockex, "~> 0.5"}
])

defmodule Chat do
  use WebSockex
  alias Kicksdk.Realtime
  alias Kicksdk.Realtime.Session

  def send_request(%{"method" => m, "url" => url, "headers" => h, "body" => body}) do
    method = String.to_existing_atom(String.downcase(m))

    res =
      Req.request!(
        method: method,
        url: url,
        headers: h,
        body: body,
        decode_body: false,
        retry: false
      )

    {res.status, res.body}
  end

  def start(slug) do
    {status, body} = send_request(Kicksdk.Website.channel_request(slug))
    {:ok, channel} = Kicksdk.Website.parse_channel(status, body)

    client_id = Base.encode16(:crypto.strong_rand_bytes(16), case: :lower)
    {s, b} = send_request(Realtime.chat_connection_request(channel["id"], client_id))
    {:ok, [%{"url" => url} | _]} = Realtime.parse_connection(s, b)
    {s, b} = send_request(Realtime.token_request(client_id))
    {:ok, token} = Realtime.parse_token(s, b)

    session = Session.new(Realtime.chat_channels(channel["chatroom_id"], channel["id"]))
    state = %{session: session, token: token}

    WebSockex.start_link(url, __MODULE__, state,
      extra_headers: [{"Origin", Realtime.required_origin()}]
    )
  end

  def handle_connect(_conn, state) do
    send(self(), {:actions, Session.open(state.session, state.token, now())})
    {:ok, state}
  end

  def handle_frame({:text, text}, state),
    do: handle_info({:actions, Session.receive(state.session, text, now())}, state)

  def handle_info({:actions, actions}, state) do
    for %{"action" => "event", "event" => %{"type" => "chat_message"} = m} <- actions,
        do: IO.puts("#{m["sender"]["username"]}: #{m["content"]}")

    handle_info(
      {:frames, for(%{"action" => "send", "text" => t} <- actions, do: {:text, t})},
      state
    )
  end

  # WebSockex sends one frame per callback: the rest wait in the mailbox.
  def handle_info({:frames, []}, state), do: {:ok, state}

  def handle_info({:frames, [frame | rest]}, state) do
    send(self(), {:frames, rest})
    {:reply, frame, state}
  end

  defp now, do: System.monotonic_time(:millisecond) - :persistent_term.get(:t0)
end

:persistent_term.put(:t0, System.monotonic_time(:millisecond))
{:ok, _} = Chat.start(List.first(System.argv(), "xqc"))
Process.sleep(:infinity)
