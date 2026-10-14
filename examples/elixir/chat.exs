# Prints a Kick channel's chat:  elixir chat.exs <channel>
#
# kicksdk builds the requests and speaks the protocol; Req sends the requests
# and WebSockex holds the socket. Builds the NIF from source (needs Rust).
#
# Everything a client that stays connected has to do, it does (docs/KICK.md
# §6.1): tick the session at its deadline (a silent socket is a dead one),
# fetch a new token when the session asks, close when it says so, and
# reconnect after a jittered backoff. It never gives up by itself: after a
# terminal close (a refused token, a ban, a connection limit) it waits 5
# minutes - longer if it happens again, up to 30 - and comes back as a new
# client, with a new client id and token. It stops only when told to
# (`Chat.stop/1`; here, Ctrl-C ends the script).
#
# KICK_WEBSITE_URL and KICK_REALTIME_URL point it at a simulator instead.

Mix.install([
  {:kicksdk, path: Path.expand("../../bindings/elixir", __DIR__)},
  {:rustler, "~> 0.38"},
  {:req, "~> 0.5"},
  {:websockex, "~> 0.5"}
])

defmodule Chat do
  use WebSockex
  alias Kicksdk.Realtime
  alias Kicksdk.Realtime.{Backoff, Session}

  def start(slug) do
    {:ok, request} = Kicksdk.Website.channel_request(slug, endpoints())
    {status, body} = fetch(request)
    {:ok, channel} = Kicksdk.Website.parse_channel(status, body)
    feeds = Realtime.chat_channels(channel["chatroom_id"], channel["id"])

    state = %{
      channel_id: channel["id"],
      session: Session.new(feeds),
      chat_feed: hd(feeds),
      client_id: nil,
      token: nil,
      backoff: Backoff.new(),
      # Counts connections: what was meant for an earlier socket (frames
      # still to send, a tick, a token being fetched) is dropped.
      gen: 0,
      outbox: [],
      timer: nil,
      # The session's "close" action, until the socket is down.
      close: nil,
      # The next connection must be a new client (after a terminal close).
      new_client: false,
      stopping: false
    }

    with {:ok, url, state} <- new_client(state),
         do: WebSockex.start(url, __MODULE__, state, [handle_initial_conn_failure: true] ++ conn_opts())
  end

  @doc "Closes the connection (or ends the wait for one) and ends the process."
  def stop(pid) do
    send(pid, :stop)
    :ok
  end

  @impl true
  def handle_connect(_conn, state) do
    IO.puts("connected")
    state = %{state | gen: state.gen + 1, outbox: [], close: nil}
    run(Session.open(state.session, state.token, now()), state)
  end

  @impl true
  def handle_frame({:text, text}, state),
    do: run(Session.receive(state.session, text, now()), state)

  def handle_frame(_frame, state), do: {:ok, state}

  @impl true
  def handle_info({:tick, gen}, %{gen: gen} = state),
    do: run(Session.tick(state.session, now()), %{state | timer: nil})

  # A new token, fetched by a task (see act/2), for this connection's client.
  def handle_info({:token, gen, token}, %{gen: gen} = state),
    do: run(Session.refresh(state.session, token, now()), %{state | token: token})

  # WebSockex sends one frame per callback, so the frames wait in the outbox
  # and go one :flush at a time, in order.
  def handle_info({:flush, gen}, %{gen: gen} = state) do
    case state.outbox do
      [] ->
        {:ok, state}

      [:close | _] ->
        {:close, %{state | outbox: []}}

      [frame | rest] ->
        if rest != [], do: send(self(), {:flush, gen})
        {:reply, frame, %{state | outbox: rest}}
    end
  end

  def handle_info(:stop, state), do: {:close, %{state | stopping: true}}

  # Anything else, including a :tick, :token or :flush from an earlier
  # connection.
  def handle_info(_other, state), do: {:ok, state}

  @impl true
  def handle_disconnect(%{reason: reason}, state) do
    # Every feed is unsubscribed now; the session is ready for a new socket.
    Session.closed(state.session)
    state = %{cancel_tick(state) | outbox: []}

    # The least to wait: the session's close says, or else the code Kick
    # closed the socket with; 0 (the ordinary backoff) for a failure with
    # no code (a network error, a refused handshake).
    retry_after_ms =
      case {state.close, reason} do
        {%{"retry_after_ms" => ms}, _} -> ms
        {nil, {:remote, code, _}} when is_integer(code) and code in 0..65_535 -> Realtime.retry_after_ms(code)
        _ -> 0
      end

    IO.puts("disconnected (#{inspect(reason)})")
    state = %{state | close: nil}
    if state.stopping, do: {:ok, state}, else: retry(state, retry_after_ms)
  end

  # Waits, then reconnects. WebSockex reconnects as soon as handle_disconnect
  # returns, so the wait is here; only a stop cuts it short.
  defp retry(state, retry_after_ms) do
    {delay, backoff} = Backoff.next_delay_for(state.backoff, retry_after_ms, :rand.uniform())
    # More than 0 is terminal: Kick does not want this client back as it is.
    state = %{state | backoff: backoff, new_client: state.new_client or retry_after_ms > 0}
    IO.puts("reconnecting in #{delay} ms#{if state.new_client, do: " as a new client"}")

    receive do
      :stop -> {:ok, state}
    after
      delay -> reconnect(state)
    end
  end

  defp reconnect(state) do
    if state.new_client do
      case new_client(state) do
        {:ok, url, state} -> {:reconnect, WebSockex.Conn.new(url, conn_opts()), state}
        {:error, reason} -> failed(state, reason)
      end
    else
      # A new token for each connection: the last one may have run out.
      case token(state.client_id) do
        {:ok, token} -> {:reconnect, %{state | token: token}}
        {:error, reason} -> failed(state, reason)
      end
    end
  end

  defp failed(state, reason) do
    IO.puts("request failed: #{inspect(reason)}")
    retry(state, 0)
  end

  # A new client: a new client id (so a new guest token), and the chat's
  # server asked again (each channel's chat lives in a region).
  defp new_client(state) do
    client_id = Base.encode16(:crypto.strong_rand_bytes(16), case: :lower)

    with {:ok, url} <- server(state.channel_id, client_id),
         {:ok, token} <- token(client_id),
         do: {:ok, url, %{state | client_id: client_id, token: token, new_client: false}}
  end

  # Does what the session says, then sets the timer for its next deadline.
  defp run(actions, state) do
    state = Enum.reduce(actions, state, &act/2)
    out = for a <- actions, item = outgoing(a), do: item
    if state.outbox == [] and out != [], do: send(self(), {:flush, state.gen})
    {:ok, schedule(%{state | outbox: state.outbox ++ out})}
  end

  defp act(%{"action" => "event", "event" => %{"type" => "chat_message"} = m}, state) do
    IO.puts("#{m["sender"]["username"]}: #{m["content"]}")
    state
  end

  # The chat feed is back: the next failure, terminal or not, waits from the
  # start again.
  defp act(%{"action" => "subscribed", "channel" => feed}, %{chat_feed: feed} = state),
    do: %{state | backoff: Backoff.reset(state.backoff)}

  # Off this process, so frames keep flowing. If it fails, do nothing: Kick
  # closes the connection when the old token runs out, and the reconnect
  # fetches a new one.
  defp act(%{"action" => "refresh_token"}, state) do
    {me, gen, client_id} = {self(), state.gen, state.client_id}

    Task.start(fn ->
      with {:ok, token} <- token(client_id), do: send(me, {:token, gen, token})
    end)

    state
  end

  # Closed after the outbox's earlier frames; handle_disconnect reconnects.
  defp act(%{"action" => "close"} = close, state) do
    kind = if close["terminal"], do: "terminal close", else: "closing"
    IO.puts("#{kind} (#{close["code"]}): #{close["reason"]}")
    %{state | close: close}
  end

  defp act(_action, state), do: state

  defp outgoing(%{"action" => "send", "text" => text}), do: {:text, text}
  defp outgoing(%{"action" => "close"}), do: :close
  defp outgoing(_action), do: nil

  defp schedule(state) do
    state = cancel_tick(state)

    case Session.deadline(state.session) do
      nil -> state
      deadline -> %{state | timer: Process.send_after(self(), {:tick, state.gen}, max(deadline - now(), 0))}
    end
  end

  defp cancel_tick(%{timer: nil} = state), do: state

  defp cancel_tick(state) do
    Process.cancel_timer(state.timer)
    %{state | timer: nil}
  end

  defp server(channel_id, client_id) do
    {status, body} = fetch(Realtime.chat_connection_request(channel_id, client_id, endpoints()))

    with {:ok, [%{"url" => url} | _]} <- Realtime.parse_connection(status, body),
         do: {:ok, url}
  rescue
    e -> {:error, e}
  end

  defp token(client_id) do
    {status, body} = fetch(Realtime.token_request(client_id, endpoints()))
    Realtime.parse_token(status, body)
  rescue
    e -> {:error, e}
  end

  defp fetch(%{"method" => m, "url" => url, "headers" => h, "body" => body}) do
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

  # nil keeps Kick's own.
  defp endpoints,
    do: %{website: System.get_env("KICK_WEBSITE_URL"), realtime: System.get_env("KICK_REALTIME_URL")}

  # Kick's realtime server refuses any other Origin.
  defp conn_opts, do: [extra_headers: [{"Origin", Realtime.required_origin()}]]

  # The session takes any integer clock: the BEAM's monotonic one will do.
  defp now, do: System.monotonic_time(:millisecond)
end

slug = List.first(System.argv()) || raise "usage: elixir chat.exs <channel>"
{:ok, pid} = Chat.start(slug)
ref = Process.monitor(pid)

receive do
  {:DOWN, ^ref, :process, _, reason} -> IO.puts("stopped: #{inspect(reason)}")
end
