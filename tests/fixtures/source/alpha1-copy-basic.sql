CREATE TABLE public.orders (
    order_id integer PRIMARY KEY,
    order_number text NOT NULL,
    customer_code text NOT NULL,
    note text,
    empty_text text NOT NULL
);

INSERT INTO public.orders (
    order_id,
    order_number,
    customer_code,
    note,
    empty_text
)
VALUES
    (1, 'EARLY-100',  'customer-a', 'plain',             ''),
    (2, 'SECOND-200', 'repeat',     E'tab\tvalue',       'filled'),
    (3, 'THIRD-300',  'customer-c', E'line1\nline2',     'filled'),
    (4, 'MIDDLE-400', 'customer-d', NULL,                'filled'),
    (5, 'FIFTH-500',  'customer-e', '',                  'filled'),
    (6, 'SIXTH-600',  'repeat',     E'carriage\rreturn', 'filled'),
    (7, 'LATE-700',   'customer-g', E'backslash\\value', 'filled');

CLUSTER public.orders USING orders_pkey;
