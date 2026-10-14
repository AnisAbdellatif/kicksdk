# Prints a Kick channel's chat:  elixir chat.exs <channel>
#
# kicksdk builds the requests and speaks the protocol; Req sends the requests
# and WebSockex holds the socket. Builds the NIF from source (needs Rust).
#
# Everything a client that stays connected has to do, it does: tick the
# session at its deadline (a silent socket is a dead one), fetch a new token
# when the session asks, close when it says so, and reconnect after a jittered
# backoff - unless the close was one Kick would refuse again.

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
    {:ok, request} = Kicksdk.Website.channel_request(slug)
    {status, body} = fetch(request)
    {:ok, channel} = Kicksdk.Website.parse_channel(status, body)

    # A guest's client id: made up, and kept for every token of this client.
    client_id = Base.encode16(:crypto.strong_rand_bytes(16), case: :lower)
    {status, body} = fetch(Realtime.chat_connection_request(channel["id"], client_id))
    {:ok, [%{"url" => url} | _]} = Realtime.parse_connection(status, body)
    {:ok, token} = token(client_id)

    feeds = Realtime.chat_channels(channel["chatroom_id"], channel["id"])

    state = %{
      session: Session.new(feeds),
      chat_feed: hd(feeds),
      client_id: client_id,
      token: token,
      backoff: Backoff.new(),
      timer: nil,
      reconnect: true
    }

    WebSockex.start(url, __MODULE__, state,
      extra_headers: [{"Origin", Realtime.required_origin()}],
      handle_initial_conn_failure: true
    )
  end

  @impl true
  def handle_connect(_conn, state) do
    IO.puts("connected")
    run(Session.open(state.session, state.token, now()), %{state | reconnect: true})
  end

  @impl true
  def handle_frame({:text, text}, state),
    do: run(Session.receive(state.session, text, now()), state)

  def handle_frame(_frame, state), do: {:ok, state}

  @impl true
  def handle_info(:tick, state),
    do: run(Session.tick(state.session, now()), %{state | timer: nil})

  # A new token, fetched by a task (see act/2).
  def handle_info({:token, token}, state),
    do: run(Session.refresh(state.session, token, now()), %{state | token: token})

  # What to send, one item per callback: WebSockex sends one frame per
  # return, so the rest wait in the mailbox, in order.
  def handle_info({:out, []}, state), do: {:ok, state}
  def handle_info({:out, [:close | _]}, state), do: {:close, state}

  def handle_info({:out, [frame | rest]}, state) do
    send(self(), {:out, rest})
    {:reply, frame, state}
  end

  def handle_info(_other, state), do: {:ok, state}

  @impl true
  def handle_disconnect(%{reason: reason} = status, state) do
    # Every feed is unsubscribed now; the session is ready for a new socket.
    Session.closed(state.session)
    state = cancel_tick(state)

    if state.reconnect and socket_allows?(reason) do
      {delay, backoff} = Backoff.next_delay_jittered(state.backoff, :rand.uniform())
      IO.puts("disconnected (#{inspect(reason)}); reconnecting in #{delay} ms")
      # WebSockex reconnects as soon as this returns, so the wait is here;
      # the process has nothing else to do while it has no socket.
      Process.sleep(delay)

      # A new token for each connection: the last one may have run out.
      case token(state.client_id) do
        {:ok, token} -> {:reconnect, %{state | backoff: backoff, token: token}}
        {:error, _} -> handle_disconnect(status, %{state | backoff: backoff})
      end
    else
      IO.puts("disconnected for good (#{inspect(reason)})")
      {:ok, state}
    end
  end

  # Does what the session says, then sets the timer for its next deadline.
  defp run(actions, state) do
    state = Enum.reduce(actions, state, &act/2)

    case for(a <- actions, item = outgoing(a), do: item) do
      [] -> :ok
      out -> send(self(), {:out, out})
    end

    {:ok, schedule(state)}
  end

  defp act(%{"action" => "event", "event" => %{"type" => "chat_message"} = m}, state) do
    IO.puts("#{m["sender"]["username"]}: #{m["content"]}")
    state
  end

  # The chat feed is back: the next failure waits from the start again.
  defp act(%{"action" => "subscribed", "channel" => feed}, %{chat_feed: feed} = state),
    do: %{state | backoff: Backoff.reset(state.backoff)}

  # Off this process, so frames keep flowing. If it fails, do nothing: Kick
  # closes the connection when the old token runs out, and the reconnect
  # fetches a new one.
  defp act(%{"action" => "refresh_token"}, state) do
    me = self()

    Task.start(fn ->
      with {:ok, token} <- token(state.client_id), do: send(me, {:token, token})
    end)

    state
  end

  # `reconnect` false: the same connection would be refused again.
  defp act(
         %{"action" => "close", "code" => code, "reason" => reason, "reconnect" => reconnect},
         state
       ) do
    IO.puts("closing (#{code}): #{reason}")
    %{state | reconnect: reconnect}
  end

  defp act(_action, state), do: state

  defp outgoing(%{"action" => "send", "text" => text}), do: {:text, text}
  defp outgoing(%{"action" => "close"}), do: :close
  defp outgoing(_action), do: nil

  # A close code from Kick's side of the socket can be terminal too.
  defp socket_allows?({:remote, code, _}) when is_integer(code) and code in 0..65_535,
    do: Realtime.should_reconnect?(code)

  defp socket_allows?(_reason), do: true

  defp schedule(state) do
    state = cancel_tick(state)

    case Session.deadline(state.session) do
      nil -> state
      deadline -> %{state | timer: Process.send_after(self(), :tick, max(deadline - now(), 0))}
    end
  end

  defp cancel_tick(%{timer: nil} = state), do: state

  defp cancel_tick(state) do
    Process.cancel_timer(state.timer)
    %{state | timer: nil}
  end

  defp token(client_id) do
    {status, body} = fetch(Realtime.token_request(client_id))
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

  # The session takes any integer clock: the BEAM's monotonic one will do.
  defp now, do: System.monotonic_time(:millisecond)
end

slug = List.first(System.argv()) || raise "usage: elixir chat.exs <channel>"
{:ok, pid} = Chat.start(slug)
ref = Process.monitor(pid)

receive do
  {:DOWN, ^ref, :process, _, reason} -> IO.puts("stopped: #{inspect(reason)}")
end
