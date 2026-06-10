import * as React from 'react'
import { cva, type VariantProps } from 'class-variance-authority'
import { cn } from '@/lib/utils'

const buttonVariants = cva(
  'inline-flex items-center justify-center gap-1.5 whitespace-nowrap rounded-md font-medium transition-colors cursor-pointer focus-visible:outline-2 focus-visible:outline-brass-400 disabled:pointer-events-none disabled:opacity-40',
  {
    variants: {
      variant: {
        default: 'bg-brass-500 text-coal-950 hover:bg-brass-400',
        secondary: 'bg-coal-800 text-parchment-100 border border-coal-700 hover:bg-coal-700',
        ghost: 'text-parchment-300 hover:bg-coal-800 hover:text-parchment-100',
        danger: 'bg-ember-400/15 text-ember-400 border border-ember-400/30 hover:bg-ember-400/25',
      },
      size: {
        default: 'h-9 px-4 text-sm',
        sm: 'h-7 px-2.5 text-xs',
        icon: 'h-8 w-8',
      },
    },
    defaultVariants: { variant: 'default', size: 'default' },
  },
)

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {}

export const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, ...props }, ref) => (
    <button ref={ref} className={cn(buttonVariants({ variant, size }), className)} {...props} />
  ),
)
Button.displayName = 'Button'
